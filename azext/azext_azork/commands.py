# --------------------------------------------------------------------------------------------
# Copyright (c) rysweet. Licensed under the MIT License.
# --------------------------------------------------------------------------------------------
"""Command table for ``az azork``."""


def load_command_table(self, _args):
    with self.command_group("azork") as g:
        g.custom_command("play", "azork_play")
        g.custom_command("run", "azork_run")
        g.custom_command("version", "azork_version")
