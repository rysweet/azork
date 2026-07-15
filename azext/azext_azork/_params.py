# --------------------------------------------------------------------------------------------
# Copyright (c) rysweet. Licensed under the MIT License.
# --------------------------------------------------------------------------------------------
"""Argument definitions for ``az azork``."""


def load_arguments(self, _command):
    with self.argument_context("azork play") as c:
        c.argument("backend", options_list=["--backend", "-b"],
                   help="Backend to explore: 'mock' (default, offline) or 'az' (live subscription).")

    with self.argument_context("azork run") as c:
        c.argument("commands", options_list=["--commands", "-c"],
                   help="AzZork commands to run, separated by ';' or newlines.")
        c.argument("backend", options_list=["--backend", "-b"],
                   help="Backend to explore: 'mock' (default, offline) or 'az' (live subscription).")
