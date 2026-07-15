#!/usr/bin/env python
# --------------------------------------------------------------------------------------------
# Copyright (c) rysweet. Licensed under the MIT License.
# --------------------------------------------------------------------------------------------
from setuptools import setup, find_packages

VERSION = "0.2.0"

CLASSIFIERS = [
    "Development Status :: 4 - Beta",
    "Intended Audience :: Developers",
    "Intended Audience :: System Administrators",
    "Programming Language :: Python",
    "Programming Language :: Python :: 3",
    "Programming Language :: Python :: 3.8",
    "Programming Language :: Python :: 3.9",
    "Programming Language :: Python :: 3.10",
    "Programming Language :: Python :: 3.11",
    "Programming Language :: Python :: 3.12",
    "License :: OSI Approved :: MIT License",
]

setup(
    name="azork",
    version=VERSION,
    description="Microsoft Azure CLI 'azork' extension — play AzZork under `az azork`.",
    long_description=(
        "A thin Azure CLI extension that surfaces the AzZork text adventure under "
        "the `az azork` command group by shelling out to the compiled azork binary."
    ),
    license="MIT",
    author="rysweet",
    url="https://github.com/rysweet/azork",
    classifiers=CLASSIFIERS,
    packages=find_packages(),
    include_package_data=True,
    package_data={"azext_azork": ["azext_metadata.json", "bin/*"]},
    install_requires=[],
)
