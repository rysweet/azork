# --------------------------------------------------------------------------------------------
# Copyright (c) rysweet. Licensed under the MIT License.
# --------------------------------------------------------------------------------------------
"""Implementations for ``az azork`` — thin shims over the azork Rust binary."""

import os
import shutil
import subprocess
import sys

from azure.cli.core.azclierror import (
    UserFault,
    ValidationError,
)


def _find_binary():
    """Locate the azork binary.

    Search order:
      1. ``AZORK_BIN`` environment variable (explicit override).
      2. A binary bundled next to this extension (``bin/azork``).
      3. ``azork`` on ``PATH``.
    """
    override = os.environ.get("AZORK_BIN")
    if override:
        if os.path.isfile(override) and os.access(override, os.X_OK):
            return override
        raise ValidationError(
            "AZORK_BIN is set to '{}' but it is not an executable file.".format(override)
        )

    bundled = os.path.join(os.path.dirname(__file__), "bin", "azork")
    if os.path.isfile(bundled) and os.access(bundled, os.X_OK):
        return bundled

    found = shutil.which("azork")
    if found:
        return found

    raise UserFault(
        "Could not find the 'azork' binary. Build it with 'cargo build --release' "
        "in the azork repo and either add it to PATH or set AZORK_BIN to its path."
    )


def _backend_args(backend):
    if not backend:
        return []
    backend = backend.strip().lower()
    if backend not in ("mock", "az"):
        raise ValidationError("--backend must be 'mock' or 'az' (got '{}').".format(backend))
    return ["--backend", backend]


def azork_play(backend=None):
    """Launch AzZork interactively, attached to the current terminal."""
    binary = _find_binary()
    cmd = [binary] + _backend_args(backend)
    # Inherit stdio so the REPL is fully interactive.
    completed = subprocess.run(cmd)  # noqa: S603
    if completed.returncode != 0:
        sys.exit(completed.returncode)


def azork_run(commands, backend=None):
    """Feed one or more commands to the AzZork REPL and return its narration."""
    binary = _find_binary()
    # Accept ';' or newline separators; normalise to newline-delimited stdin.
    script_lines = []
    for chunk in commands.replace(";", "\n").splitlines():
        line = chunk.strip()
        if line:
            script_lines.append(line)
    stdin_text = "\n".join(script_lines) + "\n"

    cmd = [binary] + _backend_args(backend)
    proc = subprocess.run(  # noqa: S603
        cmd,
        input=stdin_text,
        capture_output=True,
        text=True,
    )
    if proc.returncode != 0 and proc.stderr:
        raise UserFault("azork exited with code {}: {}".format(proc.returncode, proc.stderr.strip()))
    # Return the narration as a plain string so `az` prints it verbatim.
    return proc.stdout


def azork_version():
    """Report the azork binary path and version banner."""
    binary = _find_binary()
    proc = subprocess.run(  # noqa: S603
        [binary],
        input="version\nquit\n",
        capture_output=True,
        text=True,
    )
    return {
        "binary": binary,
        "output": proc.stdout.strip(),
    }
