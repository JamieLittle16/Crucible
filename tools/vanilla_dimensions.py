#!/usr/bin/env python3
"""Pinned vanilla dimension descriptors for cold Crucible qualification tooling.

This module is deliberately small.  It describes target-visible dimension identity and
save topology; it does not model Crucible's future runtime Dimension object and must not
be imported by production server code.
"""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True, slots=True)
class VanillaDimensionDescriptor:
    """Stable identity and 26.2 save topology for one standard vanilla dimension."""

    key: str
    region_path: Path


STANDARD_DIMENSIONS: tuple[VanillaDimensionDescriptor, ...] = (
    VanillaDimensionDescriptor(
        key="minecraft:overworld",
        region_path=Path("dimensions/minecraft/overworld/region"),
    ),
    VanillaDimensionDescriptor(
        key="minecraft:the_nether",
        region_path=Path("dimensions/minecraft/the_nether/region"),
    ),
    VanillaDimensionDescriptor(
        key="minecraft:the_end",
        region_path=Path("dimensions/minecraft/the_end/region"),
    ),
)

BY_KEY = {descriptor.key: descriptor for descriptor in STANDARD_DIMENSIONS}

if len(BY_KEY) != len(STANDARD_DIMENSIONS):
    raise RuntimeError("duplicate standard vanilla dimension key")


def require_standard_dimension(key: str) -> VanillaDimensionDescriptor:
    """Return a pinned standard descriptor or fail rather than inventing topology."""

    try:
        return BY_KEY[key]
    except KeyError as error:
        raise ValueError(f"unsupported standard vanilla dimension: {key}") from error
