# --------------------------------------------------------------------------------------------
# Copyright (c) rysweet. Licensed under the MIT License.
# --------------------------------------------------------------------------------------------
"""azext_azork: an Azure CLI extension that surfaces AzZork under ``az azork``.

The extension is a thin Python shim: every command shells out to the compiled
``azork`` Rust binary. The heavy lifting (capability derivation, graph memory,
intent resolution) lives in the binary; this module only wires the binary into
the ``az`` command tree.
"""

from azure.cli.core import AzCommandsLoader

from azext_azork._help import helps  # noqa: F401  (registers help on import)


class AzorkCommandsLoader(AzCommandsLoader):
    """Loader that registers the ``az azork`` command group."""

    def __init__(self, cli_ctx=None):
        from azure.cli.core.commands import CliCommandType

        azork_custom = CliCommandType(operations_tmpl="azext_azork.custom#{}")
        super().__init__(cli_ctx=cli_ctx, custom_command_type=azork_custom)

    def load_command_table(self, args):
        from azext_azork.commands import load_command_table

        load_command_table(self, args)
        return self.command_table

    def load_arguments(self, command):
        from azext_azork._params import load_arguments

        load_arguments(self, command)


COMMAND_LOADER_CLS = AzorkCommandsLoader
