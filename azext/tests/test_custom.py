# --------------------------------------------------------------------------------------------
# Copyright (c) rysweet. Licensed under the MIT License.
# --------------------------------------------------------------------------------------------
"""Outside-in tests for the `az azork` CLI extension (`azext_azork.custom`).

These are the PR's QA evidence for the **azext_azork CLI extension** surface,
substituting for `gadugi-test` (documented as unavailable in this environment
in the PR description). Two layers are covered:

1. Unit-level, mocked-subprocess tests that pin down the *contract* of each
   shim (`azork_play`, `azork_run`, `azork_version`, `_find_binary`,
   `_backend_args`) without needing a real `az` install or a built binary.
2. A true end-to-end test that shells out to the *actual compiled* `azork`
   binary (built via `cargo build`) through the unmodified `azork_run` /
   `azork_version` shims, exercising the real user-facing path:
   `az azork run --commands "..."` -> subprocess -> real REPL -> narration
   text returned to the caller. Skipped (not failed) if the binary hasn't
   been built yet, so this suite is safe to run before or after `cargo build`.
"""

import importlib.util
import os
import subprocess
import sys
from pathlib import Path
from unittest import mock

import pytest

REPO_ROOT_FOR_IMPORT = Path(__file__).resolve().parent.parent

# Import `custom.py` directly by file path rather than `from azext_azork import
# custom`, so this suite doesn't need the *package* `__init__.py` (which pulls
# in `azure.cli.core.AzCommandsLoader` — a much bigger surface than the
# `azclierror` stand-in in conftest.py covers, and irrelevant to the shim
# logic under test here).
_spec = importlib.util.spec_from_file_location(
    "azext_azork.custom", REPO_ROOT_FOR_IMPORT / "azext_azork" / "custom.py"
)
custom = importlib.util.module_from_spec(_spec)
sys.modules["azext_azork.custom"] = custom
_spec.loader.exec_module(custom)

REPO_ROOT = Path(__file__).resolve().parent.parent.parent
DEBUG_BINARY = REPO_ROOT / "target" / "debug" / "azork"
RELEASE_BINARY = REPO_ROOT / "target" / "release" / "azork"


def _real_binary():
    for candidate in (RELEASE_BINARY, DEBUG_BINARY):
        if candidate.is_file() and os.access(candidate, os.X_OK):
            return candidate
    return None


# ---------------------------------------------------------------------------
# Unit-level contract tests (mocked subprocess, no real binary required)
# ---------------------------------------------------------------------------


class TestBackendArgs:
    def test_no_backend_yields_no_flags(self):
        assert custom._backend_args(None) == []
        assert custom._backend_args("") == []

    def test_mock_backend(self):
        assert custom._backend_args("mock") == ["--backend", "mock"]

    def test_az_backend_case_insensitive(self):
        assert custom._backend_args("AZ") == ["--backend", "az"]

    def test_invalid_backend_raises_validation_error(self):
        with pytest.raises(custom.ValidationError):
            custom._backend_args("aws")


class TestFindBinary:
    def test_azork_bin_override_used_when_executable(self, tmp_path):
        fake = tmp_path / "azork"
        fake.write_text("#!/bin/sh\nexit 0\n")
        fake.chmod(0o755)
        with mock.patch.dict(os.environ, {"AZORK_BIN": str(fake)}):
            assert custom._find_binary() == str(fake)

    def test_azork_bin_override_non_executable_raises(self, tmp_path):
        fake = tmp_path / "azork"
        fake.write_text("not executable")
        with mock.patch.dict(os.environ, {"AZORK_BIN": str(fake)}):
            with pytest.raises(custom.ValidationError):
                custom._find_binary()

    def test_falls_back_to_path_lookup(self):
        with mock.patch.dict(os.environ, {}, clear=False):
            os.environ.pop("AZORK_BIN", None)
            with mock.patch.object(custom.os.path, "isfile", return_value=False):
                with mock.patch.object(
                    custom.shutil, "which", return_value="/usr/local/bin/azork"
                ):
                    assert custom._find_binary() == "/usr/local/bin/azork"

    def test_no_binary_found_raises_user_fault(self):
        with mock.patch.dict(os.environ, {}, clear=False):
            os.environ.pop("AZORK_BIN", None)
            with mock.patch.object(custom.os.path, "isfile", return_value=False):
                with mock.patch.object(custom.shutil, "which", return_value=None):
                    with pytest.raises(custom.UserFault):
                        custom._find_binary()


class TestAzorkRunShim:
    def test_normalises_semicolon_and_newline_separators(self):
        with mock.patch.object(custom, "_find_binary", return_value="/bin/azork"):
            with mock.patch.object(custom.subprocess, "run") as run:
                run.return_value = subprocess.CompletedProcess(
                    args=[], returncode=0, stdout="ok", stderr=""
                )
                custom.azork_run("look; score\nquit", backend="mock")
                _, kwargs = run.call_args
                assert kwargs["input"] == "look\nscore\nquit\n"

    def test_backend_flag_forwarded(self):
        with mock.patch.object(custom, "_find_binary", return_value="/bin/azork"):
            with mock.patch.object(custom.subprocess, "run") as run:
                run.return_value = subprocess.CompletedProcess(
                    args=[], returncode=0, stdout="", stderr=""
                )
                custom.azork_run("look", backend="az")
                args, _ = run.call_args
                assert args[0] == ["/bin/azork", "--backend", "az"]

    def test_nonzero_exit_with_stderr_raises_user_fault(self):
        with mock.patch.object(custom, "_find_binary", return_value="/bin/azork"):
            with mock.patch.object(custom.subprocess, "run") as run:
                run.return_value = subprocess.CompletedProcess(
                    args=[], returncode=1, stdout="", stderr="boom"
                )
                with pytest.raises(custom.UserFault):
                    custom.azork_run("look")

    def test_returns_stdout_narration(self):
        with mock.patch.object(custom, "_find_binary", return_value="/bin/azork"):
            with mock.patch.object(custom.subprocess, "run") as run:
                run.return_value = subprocess.CompletedProcess(
                    args=[], returncode=0, stdout="You are in a room.\n", stderr=""
                )
                out = custom.azork_run("look")
                assert out == "You are in a room.\n"


class TestAzorkVersionShim:
    def test_returns_binary_path_and_output(self):
        with mock.patch.object(custom, "_find_binary", return_value="/bin/azork"):
            with mock.patch.object(custom.subprocess, "run") as run:
                run.return_value = subprocess.CompletedProcess(
                    args=[], returncode=0, stdout="AzZork v0.4.0\n", stderr=""
                )
                result = custom.azork_version()
                assert result["binary"] == "/bin/azork"
                assert "AzZork" in result["output"]


class TestAzorkPlayShim:
    def test_exits_nonzero_on_failure(self):
        with mock.patch.object(custom, "_find_binary", return_value="/bin/azork"):
            with mock.patch.object(custom.subprocess, "run") as run:
                run.return_value = subprocess.CompletedProcess(args=[], returncode=3)
                with pytest.raises(SystemExit) as excinfo:
                    custom.azork_play()
                assert excinfo.value.code == 3

    def test_succeeds_silently_on_zero_exit(self):
        with mock.patch.object(custom, "_find_binary", return_value="/bin/azork"):
            with mock.patch.object(custom.subprocess, "run") as run:
                run.return_value = subprocess.CompletedProcess(args=[], returncode=0)
                custom.azork_play()  # must not raise


# ---------------------------------------------------------------------------
# True end-to-end: real compiled binary, unmodified shims, no mocking.
# ---------------------------------------------------------------------------


@pytest.mark.skipif(
    _real_binary() is None,
    reason="azork binary not built; run `cargo build` in the repo root first",
)
class TestAzorkExtensionEndToEnd:
    def test_azork_run_returns_real_narration(self):
        with mock.patch.dict(os.environ, {"AZORK_BIN": str(_real_binary())}):
            output = custom.azork_run("look; score", backend="mock")
        assert "Governance posture" in output

    def test_azork_version_reports_real_binary(self):
        with mock.patch.dict(os.environ, {"AZORK_BIN": str(_real_binary())}):
            result = custom.azork_version()
        assert result["binary"] == str(_real_binary())
        assert "AzZork" in result["output"]
