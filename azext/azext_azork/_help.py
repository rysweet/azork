# --------------------------------------------------------------------------------------------
# Copyright (c) rysweet. Licensed under the MIT License.
# --------------------------------------------------------------------------------------------
"""Help text for the ``az azork`` command group."""

from knack.help_files import helps

helps["azork"] = """
    type: group
    short-summary: Play AzZork — the Azure control plane as a Zork-style text adventure.
    long-summary: >
        AzZork reimagines your Azure subscription as an explorable dungeon: resource
        groups are rooms, resources are objects, and az capabilities are spells you
        learn at runtime. This extension shells out to the compiled `azork` binary.
        Set AZORK_BIN to point at the binary if it is not on PATH.
"""

helps["azork play"] = """
    type: command
    short-summary: Launch AzZork interactively (a REPL adventure).
    long-summary: >
        Runs the azork binary attached to your terminal. Use --backend az to explore
        your live subscription (read-only navigation), or the default mock estate.
    examples:
      - name: Play the offline mock estate
        text: az azork play
      - name: Explore your live subscription
        text: az azork play --backend az
"""

helps["azork run"] = """
    type: command
    short-summary: Run one or more AzZork commands non-interactively and print the output.
    long-summary: >
        Feeds the given commands to the azork REPL over stdin and returns whatever
        AzZork narrates. Separate multiple commands with ';' or newlines.
    examples:
      - name: Look around and check the score
        text: az azork run --commands "look; score"
      - name: Learn the storage capabilities from the live az CLI
        text: az azork run --commands "learn storage" --backend az
"""

helps["azork version"] = """
    type: command
    short-summary: Show the version of the underlying azork binary.
"""
