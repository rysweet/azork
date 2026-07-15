# --------------------------------------------------------------------------------------------
# Copyright (c) rysweet. Licensed under the MIT License.
# --------------------------------------------------------------------------------------------
"""Test-collection bootstrap for the ``azext_azork`` outside-in test suite.

``azext_azork.custom`` imports ``azure.cli.core.azclierror`` because it is a
thin Azure CLI extension. ``azure-cli-core`` is a large, network-installed
package that is NOT available in this environment (no `gadugi-test`, no
`azure-cli` install) — see the PR description's QA section for that documented
blocker. Rather than skip testing the extension's own logic (the outside-in
surface this task requires evidence for), we install a minimal stand-in module
for ``azure.cli.core.azclierror`` before ``azext_azork.custom`` is imported so
the *actual* shim logic (binary discovery, argument shaping, subprocess
invocation, error translation) is exercised for real.
"""

import sys
import types


def _install_stub_azure_cli_core():
    if "azure.cli.core.azclierror" in sys.modules:
        return

    azure_mod = sys.modules.setdefault("azure", types.ModuleType("azure"))
    cli_mod = types.ModuleType("azure.cli")
    core_mod = types.ModuleType("azure.cli.core")
    azclierror_mod = types.ModuleType("azure.cli.core.azclierror")

    class CLIError(Exception):
        """Stand-in base matching azure-cli-core's error hierarchy shape."""

    class UserFault(CLIError):
        pass

    class ValidationError(CLIError):
        pass

    azclierror_mod.UserFault = UserFault
    azclierror_mod.ValidationError = ValidationError

    azure_mod.cli = cli_mod
    cli_mod.core = core_mod
    core_mod.azclierror = azclierror_mod

    sys.modules["azure"] = azure_mod
    sys.modules["azure.cli"] = cli_mod
    sys.modules["azure.cli.core"] = core_mod
    sys.modules["azure.cli.core.azclierror"] = azclierror_mod


_install_stub_azure_cli_core()
