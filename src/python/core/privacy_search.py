"""Deterministic privacy boundary for abstract external-search intents.

This module deliberately owns no network client.  A search callable is injected
at the boundary, which keeps the existing offline runtime intact while making
the ordering contract testable: every abstract query is checked before the
first search call is allowed.
"""

from __future__ import annotations

from collections import deque
from dataclasses import dataclass, field
from typing import Callable, Iterable, TypeAlias
import unicodedata


MAX_ABSTRACT_QUERIES = 16
MAX_QUERY_BYTES = 4096
MAX_TOTAL_QUERY_BYTES = 16 * 1024
MAX_PII_TERMS = 100_000
MAX_PII_TERM_BYTES = 4096


class EgressViolationError(RuntimeError):
    """Raised before egress when a query contains protected local context."""


@dataclass(slots=True)
class _AutomatonNode:
    transitions: dict[str, int] = field(default_factory=dict)
    failure: int = 0
    terminal: bool = False


def _normalize_for_matching(value: str) -> str:
    """Normalize compatibility forms and case without altering outbound text."""
    return unicodedata.normalize("NFKC", value).casefold()


def _validate_query_text(query: object, *, field_name: str) -> str:
    if type(query) is not str:
        raise TypeError(f"{field_name} must be a string")
    cleaned = query.strip()
    if not cleaned:
        raise ValueError(f"{field_name} must not be empty")
    if len(cleaned.encode("utf-8")) > MAX_QUERY_BYTES:
        raise ValueError(f"{field_name} exceeds the allowed size")
    return cleaned


def _validate_abstract_queries(value: object) -> list[str]:
    if type(value) is not list:
        raise TypeError("abstract_queries must be a list of strings")
    if not value:
        raise ValueError("abstract_queries must not be empty")
    if len(value) > MAX_ABSTRACT_QUERIES:
        raise ValueError("abstract_queries exceeds the allowed count")

    queries: list[str] = []
    total_bytes = 0
    for index, query in enumerate(value):
        cleaned = _validate_query_text(
            query,
            field_name=f"abstract_queries[{index}]",
        )
        total_bytes += len(cleaned.encode("utf-8"))
        if total_bytes > MAX_TOTAL_QUERY_BYTES:
            raise ValueError("abstract_queries exceeds the total allowed size")
        queries.append(cleaned)
    return queries


@dataclass(slots=True)
class SearchIntent:
    """Strict schema containing only generalized, outbound search queries."""

    abstract_queries: list[str]

    def __post_init__(self) -> None:
        self.abstract_queries = _validate_abstract_queries(self.abstract_queries)


class EgressSanitizer:
    """A deterministic Aho-Corasick matcher for protected PKB entities."""

    def __init__(self, pii_terms: Iterable[str]) -> None:
        if isinstance(pii_terms, (str, bytes)):
            raise TypeError("pii_terms must be an iterable of strings")

        normalized_terms: set[str] = set()
        try:
            iterator = iter(pii_terms)
        except TypeError as exc:
            raise TypeError("pii_terms must be an iterable of strings") from exc

        for term in iterator:
            if type(term) is not str:
                raise TypeError("each pii_terms entry must be a string")
            cleaned = term.strip()
            if not cleaned:
                raise ValueError("pii_terms entries must not be empty")
            if len(cleaned.encode("utf-8")) > MAX_PII_TERM_BYTES:
                raise ValueError("pii_terms entry exceeds the allowed size")
            normalized_terms.add(_normalize_for_matching(cleaned))
            if len(normalized_terms) > MAX_PII_TERMS:
                raise ValueError("pii_terms exceeds the allowed count")

        self._nodes = self._build_automaton(tuple(sorted(normalized_terms)))

    @staticmethod
    def _build_automaton(patterns: tuple[str, ...]) -> tuple[_AutomatonNode, ...]:
        nodes = [_AutomatonNode()]

        for pattern in patterns:
            state = 0
            for character in pattern:
                next_state = nodes[state].transitions.get(character)
                if next_state is None:
                    next_state = len(nodes)
                    nodes[state].transitions[character] = next_state
                    nodes.append(_AutomatonNode())
                state = next_state
            nodes[state].terminal = True

        pending: deque[int] = deque()
        for child in nodes[0].transitions.values():
            pending.append(child)

        while pending:
            state = pending.popleft()
            for character, next_state in nodes[state].transitions.items():
                pending.append(next_state)
                fallback = nodes[state].failure
                while fallback and character not in nodes[fallback].transitions:
                    fallback = nodes[fallback].failure
                nodes[next_state].failure = nodes[fallback].transitions.get(
                    character,
                    0,
                )
                if nodes[nodes[next_state].failure].terminal:
                    nodes[next_state].terminal = True

        return tuple(nodes)

    def validate_query(self, query: str) -> None:
        """Hard-fail without exposing the matching entity or query in the error."""
        checked_query = _validate_query_text(query, field_name="query")
        state = 0
        for character in _normalize_for_matching(checked_query):
            while state and character not in self._nodes[state].transitions:
                state = self._nodes[state].failure
            state = self._nodes[state].transitions.get(character, 0)
            if self._nodes[state].terminal:
                raise EgressViolationError(
                    "external search query contains protected local context"
                )

    def sanitize(self, query: str) -> str:
        """Compatibility helper for hard-fail sanitization policies."""
        self.validate_query(query)
        return query


SearchResult: TypeAlias = str | dict[str, str]
SearchEngine: TypeAlias = Callable[[str], list[SearchResult]]


def mock_search_engine(query: str) -> list[SearchResult]:
    """Return a deterministic local response; this function performs no I/O."""
    return [
        {
            "title": "Mock external-search result",
            "url": "urn:mock:privacy-search",
            "snippet": f"Mock result for abstract query: {query}",
        }
    ]


class PrivacySearchPipeline:
    """Validate an entire intent before invoking an injected search engine."""

    def __init__(
        self,
        *,
        sanitizer: EgressSanitizer,
        searcher: SearchEngine = mock_search_engine,
    ) -> None:
        if not isinstance(sanitizer, EgressSanitizer):
            raise TypeError("sanitizer must be an EgressSanitizer")
        if not callable(searcher):
            raise TypeError("searcher must be callable")
        self._sanitizer = sanitizer
        self._searcher = searcher

    def retrieve(self, intent: SearchIntent | str) -> list[SearchResult]:
        if type(intent) is str:
            intent = SearchIntent(abstract_queries=[intent])
        elif not isinstance(intent, SearchIntent):
            raise TypeError("intent must be a SearchIntent or abstract query string")

        # Revalidate and snapshot the mutable schema field at the trust boundary.
        queries = _validate_abstract_queries(intent.abstract_queries)

        # All-or-nothing preflight: no earlier safe query may leave the process
        # before a later query containing protected context has been rejected.
        for query in queries:
            self._sanitizer.validate_query(query)

        integrated: list[SearchResult] = []
        for query in queries:
            results = self._searcher(query)
            if type(results) is not list:
                raise TypeError("searcher must return a list")
            integrated.extend(results)
        return integrated
