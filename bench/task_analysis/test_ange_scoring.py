"""Tests of scoring mechanics, independent of any cx output or private corpus."""
import json
from pathlib import Path
import tempfile
import unittest

from score_ange_tasks import raw_check, source_proofs


def record(rows=(), warnings=(), definitions=()):
    return {"arm": "C", "primary_rows": list(rows), "primary_docs": [{"warnings": list(warnings)}],
            "definitions": list(definitions), "source_audits": []}


class ScoringControls(unittest.TestCase):
    def test_multiplicity_is_not_set_membership(self):
        gold = {"proofs": [{"file": "a.cpp"}], "line_counts": {"5": 2}}
        row = {"file": "a.cpp", "line": 5, "to": ""}
        self.assertFalse(raw_check(record([row]), gold, False)[0])
        self.assertTrue(raw_check(record([row, row]), gold, False)[0])
        self.assertFalse(raw_check(record([row, row, row]), gold, False)[0])

    def test_matching_count_at_wrong_file_does_not_pass(self):
        gold = {"proofs": [{"file": "a.cpp"}], "line_counts": {"5": 1}}
        wrong = {"file": "b.cpp", "line": 5, "to": ""}
        self.assertFalse(raw_check(record([wrong]), gold, False)[0])

    def test_empty_success_without_unsupported_disclosure_is_not_evidence(self):
        gold = {"proofs": [{"file": "a.py"}], "require_unsupported_or_source": True}
        self.assertFalse(raw_check(record(), gold, False)[0])
        warning = "relation_coverage: " + json.dumps({"issue_counts": {"unsupported_language": 1},
                                                     "complete_within_model": False})
        self.assertTrue(raw_check(record(warnings=[warning]), gold, False)[0])

    def test_overload_union_is_not_cured_by_adding_warning(self):
        gold = {"proofs": [{"file": "a.cpp"}], "require_subject_ambiguity": True}
        warning = "2 distinct symbols named run"
        self.assertTrue(raw_check(record(warnings=[warning]), gold, False)[0])
        row = {"file": "a.cpp", "line": 2, "to": "run"}
        self.assertFalse(raw_check(record([row], [warning]), gold, False)[0])

    def test_candidate_name_is_not_a_resolved_helper(self):
        gold = {"proofs": [{"file": "a.cpp"}], "required_names": ["leaf"]}
        row = {"file": "a.cpp", "line": 2, "to": "", "ambiguous_candidates": "leaf"}
        self.assertFalse(raw_check(record([row]), gold, False)[0])

    def test_source_must_be_original_and_cover_required_span(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "a.cpp").write_text("first\nsecond\nthird\n")
            proofs = [{"file": "a.cpp", "start": 2, "end": 3}]
            good = {"file": "a.cpp", "line": 1, "body": "first\nsecond\nthird\n"}
            self.assertTrue(source_proofs(record(definitions=[good]), proofs, root)[0])
            forged = {**good, "body": "first\nchanged\nthird\n"}
            self.assertFalse(source_proofs(record(definitions=[forged]), proofs, root)[0])
            short = {**good, "body": "first\nsecond\n"}
            self.assertFalse(source_proofs(record(definitions=[short]), proofs, root)[0])


if __name__ == "__main__":
    unittest.main()
