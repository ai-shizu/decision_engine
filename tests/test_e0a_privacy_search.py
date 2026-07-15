from __future__ import annotations

import sys
from pathlib import Path
from unittest.mock import Mock

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src" / "python"))

PII_TERMS = (
    "Atsuki Ishizu",
    "Yurika",
    "Hongo 2-chome",
)


def _load_privacy_search_api():
    """Import inside each test so the absent module produces two RED results."""
    from core.privacy_search import (  # type: ignore[import-not-found]
        EgressSanitizer,
        EgressViolationError,
        PrivacySearchPipeline,
    )

    return EgressSanitizer, EgressViolationError, PrivacySearchPipeline


def test_pii_query_hard_fails_before_search() -> None:
    EgressSanitizer, EgressViolationError, PrivacySearchPipeline = (
        _load_privacy_search_api()
    )
    searcher = Mock(
        side_effect=AssertionError("search must not run for a query containing PKB PII")
    )
    pipeline = PrivacySearchPipeline(
        sanitizer=EgressSanitizer(pii_terms=PII_TERMS),
        searcher=searcher,
    )

    with pytest.raises(EgressViolationError):
        pipeline.retrieve("Atsuki Ishizu internship preparation")

    searcher.assert_not_called()


def test_safe_abstract_query_reaches_search_mock() -> None:
    EgressSanitizer, _, PrivacySearchPipeline = _load_privacy_search_api()
    query = "graphics engineer interview tips general"
    expected_results = [
        {
            "title": "General graphics engineering interview guidance",
            "url": "https://example.invalid/graphics-interview",
            "snippet": "General preparation topics for graphics engineering interviews.",
        }
    ]
    searcher = Mock(return_value=expected_results)
    pipeline = PrivacySearchPipeline(
        sanitizer=EgressSanitizer(pii_terms=PII_TERMS),
        searcher=searcher,
    )

    results = pipeline.retrieve(query)

    searcher.assert_called_once_with(query)
    assert results == expected_results
