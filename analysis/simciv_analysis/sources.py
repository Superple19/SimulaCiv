"""Compatibility exports for source-map helpers merged into claims.py."""

from .claims import load_sources, source_for_claim, validate_cited_harnesses, validate_source_paths

__all__ = ["load_sources", "source_for_claim", "validate_cited_harnesses", "validate_source_paths"]
