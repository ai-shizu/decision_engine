"""PKB core business logic (UI-independent)."""

from .offline_runtime import enforce_offline_environment

enforce_offline_environment()

from .paths import PROJECT_ROOT

__all__ = ["PROJECT_ROOT"]
