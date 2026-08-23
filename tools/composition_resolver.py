#!/usr/bin/env python3
"""Deterministic cold-path package/profile resolver for Crucible compositions."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import sys
import tomllib
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable

SCHEMA = 1
ENGINE_SPI_VERSION = 1
CAPABILITY = re.compile(
    r"^(?P<name>[a-z][a-z0-9-]*(?:\.[a-z0-9-]+)*)/(?P<version>[1-9][0-9]*)$"
)
CAPABILITY_NAME = re.compile(r"^[a-z][a-z0-9-]*(?:\.[a-z0-9-]+)*$")
PACKAGE_ID = re.compile(r"^[a-z][a-z0-9-]*$")
VERSION = re.compile(r"^[0-9]+\.[0-9]+\.[0-9]+(?:[-+][0-9A-Za-z.-]+)?$")
RUST_IDENT = re.compile(r"^[A-Z][A-Za-z0-9_]*$")
RUST_PATH = re.compile(
    r"^[A-Za-z_][A-Za-z0-9_]*(?:::[A-Za-z_][A-Za-z0-9_]*)+$"
)
CRATE_IDENT = re.compile(r"^[a-z][a-z0-9_-]*$")
PACKAGE_KINDS = {"engine-source", "native-module", "wasm-component", "data-package"}
TRUST_CLASSES = {"data-only", "sandboxed", "trusted-native", "engine-native"}
FIDELITIES = {"strict", "relaxed"}
COST_CLASSES = {"hot", "warm", "cold"}
CARDINALITIES = {"exactly-one", "many"}
OPTIMIZE_CLASSES = {"debuggability", "balanced", "performance", "memory"}


class CompositionError(RuntimeError):
    """Raised when a composition cannot be admitted deterministically."""


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def canonical_digest(value: object) -> str:
    encoded = json.dumps(
        value, sort_keys=True, separators=(",", ":"), ensure_ascii=True
    ).encode("utf-8")
    return sha256_bytes(encoded)


def _object(value: object, label: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise CompositionError(f"{label} must be a TOML table")
    return value


def _keys(
    table: dict[str, Any], *, allowed: set[str], required: set[str], label: str
) -> None:
    unknown = sorted(set(table) - allowed)
    missing = sorted(required - set(table))
    if unknown:
        raise CompositionError(f"{label} contains unknown keys: {', '.join(unknown)}")
    if missing:
        raise CompositionError(f"{label} is missing required keys: {', '.join(missing)}")


def _string(value: object, label: str) -> str:
    if not isinstance(value, str) or not value:
        raise CompositionError(f"{label} must be a non-empty string")
    return value


def _bool(value: object, label: str) -> bool:
    if type(value) is not bool:
        raise CompositionError(f"{label} must be a boolean")
    return value


def _int(value: object, label: str) -> int:
    if type(value) is not int:
        raise CompositionError(f"{label} must be an integer")
    return value


def _strings(value: object, label: str) -> tuple[str, ...]:
    if not isinstance(value, list) or any(
        not isinstance(item, str) or not item for item in value
    ):
        raise CompositionError(f"{label} must be an array of non-empty strings")
    if len(set(value)) != len(value):
        raise CompositionError(f"{label} must not contain duplicates")
    return tuple(value)


def _enum(value: object, allowed: set[str], label: str) -> str:
    text = _string(value, label)
    if text not in allowed:
        raise CompositionError(f"{label} must be one of {sorted(allowed)}, got {text!r}")
    return text


def _relative_repo_path(repo_root: Path, raw: object, label: str) -> Path:
    text = _string(raw, label)
    path = Path(text)
    if path.is_absolute() or ".." in path.parts:
        raise CompositionError(f"{label} must be a repository-relative path without '..'")
    resolved = (repo_root / path).resolve()
    try:
        resolved.relative_to(repo_root.resolve())
    except ValueError as error:
        raise CompositionError(f"{label} escapes repository root") from error
    return resolved


@dataclass(frozen=True, order=True)
class Capability:
    name: str
    version: int

    @classmethod
    def parse(cls, value: object, label: str) -> "Capability":
        text = _string(value, label)
        match = CAPABILITY.fullmatch(text)
        if match is None:
            raise CompositionError(
                f"{label} must use canonical <name>/<positive-version> syntax"
            )
        return cls(match.group("name"), int(match.group("version")))

    @property
    def identity(self) -> str:
        return f"{self.name}/{self.version}"


@dataclass(frozen=True)
class Provide:
    capability: Capability
    cardinality: str
    cost: str
    rust_export: str
    rust_type: str


@dataclass(frozen=True)
class Component:
    manifest_path: Path
    manifest_sha256: str
    package_id: str
    version: str
    kind: str
    trust: str
    fidelity: str
    qualified: bool
    crate: str
    crate_path: Path
    rust_crate: str
    semantic_deviations: tuple[str, ...]
    qualification_records: tuple[Path, ...]
    minecraft: tuple[str, ...]
    provides: tuple[Provide, ...]
    requires: tuple[Capability, ...]


@dataclass(frozen=True)
class Profile:
    path: Path
    sha256: str
    minecraft: str
    name: str
    fidelity: str
    optimize: str
    allow_unqualified: bool
    allow_third_party_native: bool
    allow_semantic_deviations: bool
    engine: tuple[tuple[str, str], ...]

    @property
    def selections(self) -> dict[str, str]:
        return dict(self.engine)


@dataclass(frozen=True)
class Resolution:
    repo_root: Path
    profile: Profile
    components: tuple[Component, ...]
    providers: tuple[tuple[Capability, str, str, str, str], ...]
    rust_toolchain: str
    generated_data_sha256: str
    composition_sha256: str


def _load_toml(path: Path) -> tuple[dict[str, Any], bytes]:
    if path.is_symlink() or not path.is_file():
        raise CompositionError(f"configuration input must be a real non-symlink file: {path}")
    data = path.read_bytes()
    try:
        value = tomllib.loads(data.decode("utf-8"))
    except (UnicodeDecodeError, tomllib.TOMLDecodeError) as error:
        raise CompositionError(f"invalid TOML in {path}: {error}") from error
    return _object(value, str(path)), data


def parse_profile(repo_root: Path, path: Path) -> Profile:
    table, raw = _load_toml(path)
    _keys(
        table,
        allowed={"minecraft", "profile", "security", "engine"},
        required={"minecraft", "profile", "security"},
        label=f"profile {path}",
    )
    minecraft = _string(table["minecraft"], f"{path}: minecraft")
    profile = _object(table["profile"], f"{path}: [profile]")
    _keys(
        profile,
        allowed={"name", "fidelity", "optimize"},
        required={"name", "fidelity", "optimize"},
        label=f"{path}: [profile]",
    )
    security = _object(table["security"], f"{path}: [security]")
    _keys(
        security,
        allowed={
            "allow_unqualified",
            "allow_third_party_native",
            "allow_semantic_deviations",
        },
        required={
            "allow_unqualified",
            "allow_third_party_native",
            "allow_semantic_deviations",
        },
        label=f"{path}: [security]",
    )
    engine_table = _object(table.get("engine", {}), f"{path}: [engine]")
    engine: list[tuple[str, str]] = []
    for capability_name, provider in sorted(engine_table.items()):
        if CAPABILITY_NAME.fullmatch(capability_name) is None:
            raise CompositionError(
                f"{path}: engine capability {capability_name!r} is not canonical"
            )
        provider_id = _string(provider, f"{path}: engine.{capability_name}")
        if PACKAGE_ID.fullmatch(provider_id) is None:
            raise CompositionError(f"{path}: invalid provider package id {provider_id!r}")
        engine.append((capability_name, provider_id))
    return Profile(
        path=path.resolve(),
        sha256=sha256_bytes(raw),
        minecraft=minecraft,
        name=_string(profile["name"], f"{path}: profile.name"),
        fidelity=_enum(
            profile["fidelity"], FIDELITIES, f"{path}: profile.fidelity"
        ),
        optimize=_enum(
            profile["optimize"], OPTIMIZE_CLASSES, f"{path}: profile.optimize"
        ),
        allow_unqualified=_bool(
            security["allow_unqualified"], f"{path}: security.allow_unqualified"
        ),
        allow_third_party_native=_bool(
            security["allow_third_party_native"],
            f"{path}: security.allow_third_party_native",
        ),
        allow_semantic_deviations=_bool(
            security["allow_semantic_deviations"],
            f"{path}: security.allow_semantic_deviations",
        ),
        engine=tuple(engine),
    )


def parse_component(repo_root: Path, path: Path) -> Component:
    table, raw = _load_toml(path)
    _keys(
        table,
        allowed={"schema", "package", "compatibility", "provides", "requires"},
        required={"schema", "package", "compatibility", "provides"},
        label=f"component {path}",
    )
    if _int(table["schema"], f"{path}: schema") != SCHEMA:
        raise CompositionError(f"{path}: unsupported component schema")
    package = _object(table["package"], f"{path}: [package]")
    _keys(
        package,
        allowed={
            "id",
            "version",
            "kind",
            "trust",
            "fidelity",
            "qualified",
            "crate",
            "crate_path",
            "rust_crate",
            "semantic_deviations",
            "qualification_records",
        },
        required={
            "id",
            "version",
            "kind",
            "trust",
            "fidelity",
            "qualified",
            "crate",
            "crate_path",
            "rust_crate",
            "semantic_deviations",
            "qualification_records",
        },
        label=f"{path}: [package]",
    )
    package_id = _string(package["id"], f"{path}: package.id")
    if PACKAGE_ID.fullmatch(package_id) is None:
        raise CompositionError(f"{path}: invalid package id {package_id!r}")
    version = _string(package["version"], f"{path}: package.version")
    if VERSION.fullmatch(version) is None:
        raise CompositionError(f"{path}: package.version is not canonical semver")
    crate = _string(package["crate"], f"{path}: package.crate")
    if CRATE_IDENT.fullmatch(crate) is None:
        raise CompositionError(f"{path}: invalid Cargo package name {crate!r}")
    rust_crate = _string(package["rust_crate"], f"{path}: package.rust_crate")
    if re.fullmatch(r"[a-z][a-z0-9_]*", rust_crate) is None:
        raise CompositionError(f"{path}: invalid Rust crate identifier {rust_crate!r}")
    crate_path = _relative_repo_path(
        repo_root, package["crate_path"], f"{path}: package.crate_path"
    )
    if not (crate_path / "Cargo.toml").is_file():
        raise CompositionError(f"{path}: package.crate_path has no Cargo.toml: {crate_path}")
    deviations = _strings(
        package["semantic_deviations"], f"{path}: package.semantic_deviations"
    )
    record_texts = _strings(
        package["qualification_records"], f"{path}: package.qualification_records"
    )
    qualification_records: list[Path] = []
    for raw_record in record_texts:
        record = _relative_repo_path(
            repo_root, raw_record, f"{path}: qualification record"
        )
        if record.is_symlink() or not record.is_file():
            raise CompositionError(
                f"{path}: qualification record does not exist: {raw_record}"
            )
        qualification_records.append(record)
    qualified = _bool(package["qualified"], f"{path}: package.qualified")
    if qualified and not qualification_records:
        raise CompositionError(
            f"{path}: qualified package must cite local qualification records"
        )

    compatibility = _object(table["compatibility"], f"{path}: [compatibility]")
    _keys(
        compatibility,
        allowed={"minecraft"},
        required={"minecraft"},
        label=f"{path}: [compatibility]",
    )
    minecraft = _strings(
        compatibility["minecraft"], f"{path}: compatibility.minecraft"
    )

    provides_raw = table["provides"]
    if not isinstance(provides_raw, list) or not provides_raw:
        raise CompositionError(
            f"{path}: [[provides]] must be a non-empty array of tables"
        )
    provides: list[Provide] = []
    seen_capabilities: set[Capability] = set()
    seen_exports: set[str] = set()
    for index, raw_provide in enumerate(provides_raw):
        provide = _object(raw_provide, f"{path}: provides[{index}]")
        _keys(
            provide,
            allowed={
                "capability",
                "cardinality",
                "cost",
                "rust_export",
                "rust_type",
            },
            required={
                "capability",
                "cardinality",
                "cost",
                "rust_export",
                "rust_type",
            },
            label=f"{path}: provides[{index}]",
        )
        capability = Capability.parse(
            provide["capability"], f"{path}: provides[{index}].capability"
        )
        if capability in seen_capabilities:
            raise CompositionError(
                f"{path}: duplicate provided capability {capability.identity}"
            )
        seen_capabilities.add(capability)
        rust_export = _string(
            provide["rust_export"], f"{path}: provides[{index}].rust_export"
        )
        rust_type = _string(
            provide["rust_type"], f"{path}: provides[{index}].rust_type"
        )
        if RUST_IDENT.fullmatch(rust_export) is None:
            raise CompositionError(f"{path}: invalid Rust export name {rust_export!r}")
        if rust_export in seen_exports:
            raise CompositionError(f"{path}: duplicate Rust export {rust_export}")
        seen_exports.add(rust_export)
        if RUST_PATH.fullmatch(rust_type) is None or not rust_type.startswith(
            f"{rust_crate}::"
        ):
            raise CompositionError(
                f"{path}: rust_type must be a path rooted at package.rust_crate"
            )
        provides.append(
            Provide(
                capability=capability,
                cardinality=_enum(
                    provide["cardinality"],
                    CARDINALITIES,
                    f"{path}: provides[{index}].cardinality",
                ),
                cost=_enum(
                    provide["cost"],
                    COST_CLASSES,
                    f"{path}: provides[{index}].cost",
                ),
                rust_export=rust_export,
                rust_type=rust_type,
            )
        )

    requires_raw = table.get("requires", [])
    if not isinstance(requires_raw, list):
        raise CompositionError(f"{path}: [[requires]] must be an array of tables")
    requires: list[Capability] = []
    for index, raw_require in enumerate(requires_raw):
        require = _object(raw_require, f"{path}: requires[{index}]")
        _keys(
            require,
            allowed={"capability"},
            required={"capability"},
            label=f"{path}: requires[{index}]",
        )
        requires.append(
            Capability.parse(
                require["capability"], f"{path}: requires[{index}].capability"
            )
        )
    if len(set(requires)) != len(requires):
        raise CompositionError(f"{path}: duplicate required capability")

    return Component(
        manifest_path=path.resolve(),
        manifest_sha256=sha256_bytes(raw),
        package_id=package_id,
        version=version,
        kind=_enum(package["kind"], PACKAGE_KINDS, f"{path}: package.kind"),
        trust=_enum(package["trust"], TRUST_CLASSES, f"{path}: package.trust"),
        fidelity=_enum(
            package["fidelity"], FIDELITIES, f"{path}: package.fidelity"
        ),
        qualified=qualified,
        crate=crate,
        crate_path=crate_path,
        rust_crate=rust_crate,
        semantic_deviations=deviations,
        qualification_records=tuple(qualification_records),
        minecraft=minecraft,
        provides=tuple(provides),
        requires=tuple(requires),
    )


def load_components(repo_root: Path, components_root: Path) -> tuple[Component, ...]:
    if components_root.is_symlink() or not components_root.is_dir():
        raise CompositionError(
            f"components root must be a real directory: {components_root}"
        )
    paths = sorted(components_root.rglob("component.toml"))
    if not paths:
        raise CompositionError(f"no component manifests found below {components_root}")
    components = tuple(parse_component(repo_root, path) for path in paths)
    ids = [component.package_id for component in components]
    if len(set(ids)) != len(ids):
        raise CompositionError("component package ids must be globally unique")
    return components


def _admit_component(component: Component, profile: Profile) -> None:
    if profile.minecraft not in component.minecraft:
        raise CompositionError(
            f"{component.package_id} does not support Minecraft {profile.minecraft}"
        )
    if profile.fidelity == "strict" and component.fidelity != "strict":
        raise CompositionError(
            f"strict profile cannot select relaxed component {component.package_id}"
        )
    if not profile.allow_unqualified and not component.qualified:
        raise CompositionError(
            f"profile forbids unqualified component {component.package_id}"
        )
    if not profile.allow_semantic_deviations and component.semantic_deviations:
        raise CompositionError(
            f"profile forbids semantic deviations from {component.package_id}: "
            + ", ".join(component.semantic_deviations)
        )
    if not profile.allow_third_party_native and (
        component.kind == "native-module" or component.trust == "trusted-native"
    ):
        raise CompositionError(
            f"profile forbids third-party native component {component.package_id}"
        )


def resolve(profile: Profile, components: tuple[Component, ...]) -> tuple[Component, ...]:
    by_id = {component.package_id: component for component in components}
    selections = profile.selections
    if not selections:
        raise CompositionError(
            f"profile {profile.name!r} has no engine selections and is not a runnable composition"
        )
    selected: dict[str, Component] = {}

    for capability_name, package_id in selections.items():
        component = by_id.get(package_id)
        if component is None:
            raise CompositionError(
                f"profile selects unknown provider {package_id!r} for {capability_name}"
            )
        matches = [
            provide
            for provide in component.provides
            if provide.capability.name == capability_name
        ]
        if not matches:
            raise CompositionError(
                f"profile provider {package_id} does not provide {capability_name}"
            )
        _admit_component(component, profile)
        selected[component.package_id] = component

    pending = list(selected.values())
    examined: set[str] = set()
    while pending:
        component = pending.pop()
        if component.package_id in examined:
            continue
        examined.add(component.package_id)
        for requirement in component.requires:
            providers = [
                candidate
                for candidate in selected.values()
                if any(
                    provide.capability == requirement
                    for provide in candidate.provides
                )
            ]
            if providers:
                continue
            candidates = [
                candidate
                for candidate in components
                if profile.minecraft in candidate.minecraft
                and any(
                    provide.capability == requirement
                    for provide in candidate.provides
                )
            ]
            explicit_id = selections.get(requirement.name)
            if explicit_id is not None:
                candidates = [
                    candidate
                    for candidate in candidates
                    if candidate.package_id == explicit_id
                ]
            if len(candidates) != 1:
                ids = (
                    ", ".join(sorted(candidate.package_id for candidate in candidates))
                    or "none"
                )
                raise CompositionError(
                    f"requirement {requirement.identity} for {component.package_id} "
                    f"does not resolve uniquely; candidates: {ids}"
                )
            provider = candidates[0]
            _admit_component(provider, profile)
            selected[provider.package_id] = provider
            pending.append(provider)

    selected_provides: dict[Capability, list[tuple[Component, Provide]]] = {}
    for component in selected.values():
        _admit_component(component, profile)
        for provide in component.provides:
            selected_provides.setdefault(provide.capability, []).append(
                (component, provide)
            )
    for capability, entries in selected_provides.items():
        cardinalities = {provide.cardinality for _, provide in entries}
        if len(cardinalities) != 1:
            raise CompositionError(
                f"providers disagree on cardinality for {capability.identity}"
            )
        cardinality = next(iter(cardinalities))
        if cardinality == "exactly-one" and len(entries) != 1:
            raise CompositionError(
                f"capability {capability.identity} requires exactly one provider, got "
                + ", ".join(
                    sorted(component.package_id for component, _ in entries)
                )
            )

    for capability_name, package_id in selections.items():
        matches = [
            capability
            for capability, entries in selected_provides.items()
            if capability.name == capability_name
            and any(
                component.package_id == package_id for component, _ in entries
            )
        ]
        if len(matches) != 1:
            raise CompositionError(
                f"profile selection {capability_name} -> {package_id} is version-ambiguous"
            )

    return tuple(
        sorted(selected.values(), key=lambda component: component.package_id)
    )


def _read_toolchain(repo_root: Path) -> str:
    table, _ = _load_toml(repo_root / "rust-toolchain.toml")
    toolchain = _object(
        table.get("toolchain"), "rust-toolchain.toml: [toolchain]"
    )
    return _string(
        toolchain.get("channel"), "rust-toolchain.toml: toolchain.channel"
    )


def _read_generated_digest(repo_root: Path, minecraft: str) -> str:
    path = repo_root / f"vanilla/state-data/{minecraft}-state-data-manifest.json"
    if path.is_symlink() or not path.is_file():
        raise CompositionError(f"missing generated target-data manifest: {path}")
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as error:
        raise CompositionError(
            f"invalid generated target-data manifest: {error}"
        ) from error
    digest = data.get("generation_digest") if isinstance(data, dict) else None
    if not isinstance(digest, str) or re.fullmatch(r"[0-9a-f]{64}", digest) is None:
        raise CompositionError(
            "generated target-data generation_digest is malformed"
        )
    target = data.get("target")
    if not isinstance(target, dict) or target.get("minecraft_version") != minecraft:
        raise CompositionError(
            "generated target-data Minecraft identity does not match profile"
        )
    return digest


def build_resolution(
    *, repo_root: Path, profile_path: Path, components_root: Path
) -> Resolution:
    repo_root = repo_root.resolve()
    profile = parse_profile(repo_root, profile_path.resolve())
    components = load_components(repo_root, components_root.resolve())
    selected = resolve(profile, components)
    providers: list[tuple[Capability, str, str, str, str]] = []
    exports: set[str] = set()
    for component in selected:
        for provide in component.provides:
            if provide.rust_export in exports:
                raise CompositionError(
                    f"generated Rust export collision: {provide.rust_export}"
                )
            exports.add(provide.rust_export)
            providers.append(
                (
                    provide.capability,
                    component.package_id,
                    provide.rust_export,
                    provide.rust_type,
                    provide.cost,
                )
            )
    providers.sort(key=lambda item: item[0])
    rust_toolchain = _read_toolchain(repo_root)
    generated_data_sha = _read_generated_digest(repo_root, profile.minecraft)
    identity = {
        "schema": SCHEMA,
        "engine_spi_version": ENGINE_SPI_VERSION,
        "minecraft": profile.minecraft,
        "profile": profile.name,
        "profile_sha256": profile.sha256,
        "rust_toolchain": rust_toolchain,
        "generated_data_sha256": generated_data_sha,
        "packages": [
            {
                "id": component.package_id,
                "version": component.version,
                "manifest_sha256": component.manifest_sha256,
                "kind": component.kind,
                "trust": component.trust,
                "fidelity": component.fidelity,
                "qualified": component.qualified,
                "semantic_deviations": list(component.semantic_deviations),
                "qualification_records": [
                    str(record.relative_to(repo_root))
                    for record in component.qualification_records
                ],
            }
            for component in selected
        ],
        "providers": [
            {
                "capability": capability.identity,
                "package": package_id,
                "rust_export": rust_export,
                "rust_type": rust_type,
                "cost": cost,
            }
            for capability, package_id, rust_export, rust_type, cost in providers
        ],
    }
    return Resolution(
        repo_root=repo_root,
        profile=profile,
        components=selected,
        providers=tuple(providers),
        rust_toolchain=rust_toolchain,
        generated_data_sha256=generated_data_sha,
        composition_sha256=canonical_digest(identity),
    )


def _quote(value: str) -> str:
    return json.dumps(value, ensure_ascii=True)


def _array(values: Iterable[str]) -> str:
    return "[" + ", ".join(_quote(value) for value in values) + "]"


def render_lock(resolution: Resolution) -> str:
    profile = resolution.profile
    lines = [
        "# This file is generated by tools/composition_resolver.py. Do not edit by hand.",
        f"schema = {SCHEMA}",
        f"engine_spi_version = {ENGINE_SPI_VERSION}",
        f"composition_sha256 = {_quote(resolution.composition_sha256)}",
        f"minecraft = {_quote(profile.minecraft)}",
        f"profile = {_quote(profile.name)}",
        f"profile_sha256 = {_quote(profile.sha256)}",
        f"rust_toolchain = {_quote(resolution.rust_toolchain)}",
        f"generated_data_sha256 = {_quote(resolution.generated_data_sha256)}",
        "",
    ]
    for component in resolution.components:
        lines.extend(
            [
                "[[packages]]",
                f"id = {_quote(component.package_id)}",
                f"version = {_quote(component.version)}",
                f"manifest_sha256 = {_quote(component.manifest_sha256)}",
                f"kind = {_quote(component.kind)}",
                f"trust = {_quote(component.trust)}",
                f"fidelity = {_quote(component.fidelity)}",
                f"qualified = {'true' if component.qualified else 'false'}",
                f"semantic_deviations = {_array(component.semantic_deviations)}",
                "qualification_records = "
                + _array(
                    str(record.relative_to(resolution.repo_root))
                    for record in component.qualification_records
                ),
                "",
            ]
        )
    for capability, package_id, rust_export, rust_type, cost in resolution.providers:
        lines.extend(
            [
                "[[providers]]",
                f"capability = {_quote(capability.identity)}",
                f"package = {_quote(package_id)}",
                f"rust_export = {_quote(rust_export)}",
                f"rust_type = {_quote(rust_type)}",
                f"cost = {_quote(cost)}",
                "",
            ]
        )
    return "\n".join(lines)


def render_crate_toml(resolution: Resolution, crate_dir: Path) -> str:
    dependencies: dict[str, str] = {}
    for component in resolution.components:
        relative = os.path.relpath(
            component.crate_path, crate_dir.resolve()
        ).replace(os.sep, "/")
        existing = dependencies.get(component.crate)
        if existing is not None and existing != relative:
            raise CompositionError(
                f"Cargo package {component.crate} resolves to multiple local paths"
            )
        dependencies[component.crate] = relative
    lines = [
        "# This file is generated by tools/composition_resolver.py. Do not edit by hand.",
        "[package]",
        'name = "crucible-composition"',
        'version = "0.0.0"',
        "edition.workspace = true",
        "rust-version.workspace = true",
        "repository.workspace = true",
        "license.workspace = true",
        "publish = false",
        "",
        "[dependencies]",
    ]
    for package, relative in sorted(dependencies.items()):
        lines.append(f"{package} = {{ path = {_quote(relative)} }}")
    lines.extend(["", "[lints]", "workspace = true", ""])
    return "\n".join(lines)


def render_lib_rs(resolution: Resolution) -> str:
    lines = [
        "//! Generated static composition wiring. Regenerate with `tools/composition_resolver.py`.",
        "//!",
        "//! The generated surface deliberately re-exports concrete provider types. There is no",
        "//! runtime service map or mandatory trait-object hop in this composition boundary.",
        "",
        "#![forbid(unsafe_code)]",
        "",
        f'pub const COMPOSITION_SHA256: &str = "{resolution.composition_sha256}";',
        f'pub const PROFILE: &str = "{resolution.profile.name}";',
        f'pub const MINECRAFT_VERSION: &str = "{resolution.profile.minecraft}";',
        "",
    ]
    for _capability, _package_id, rust_export, rust_type, _cost in resolution.providers:
        lines.append(f"pub use {rust_type} as {rust_export};")
    lines.extend(
        [
            "",
            "#[cfg(test)]",
            "mod tests {",
            "    use super::{COMPOSITION_SHA256, MINECRAFT_VERSION, PROFILE};",
            "",
            "    #[test]",
            "    fn generated_identity_is_nonempty_and_pinned() {",
            "        assert_eq!(COMPOSITION_SHA256.len(), 64);",
            '        assert_eq!(MINECRAFT_VERSION, "26.2");',
            '        assert_eq!(PROFILE, "reference");',
            "    }",
            "}",
            "",
        ]
    )
    return "\n".join(lines)


def generated_files(resolution: Resolution, crate_dir: Path) -> dict[Path, str]:
    return {
        crate_dir / "Cargo.toml": render_crate_toml(resolution, crate_dir),
        crate_dir / "src/lib.rs": render_lib_rs(resolution),
    }


def generate(*, resolution: Resolution, lock_path: Path, crate_dir: Path) -> None:
    lock_path.parent.mkdir(parents=True, exist_ok=True)
    lock_path.write_text(render_lock(resolution), encoding="utf-8")
    for path, content in generated_files(resolution, crate_dir).items():
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")


def check(*, resolution: Resolution, lock_path: Path, crate_dir: Path) -> None:
    expected = {
        lock_path: render_lock(resolution),
        **generated_files(resolution, crate_dir),
    }
    drift: list[str] = []
    for path, content in expected.items():
        if path.is_symlink() or not path.is_file():
            drift.append(f"missing generated file: {path}")
            continue
        if path.read_text(encoding="utf-8") != content:
            drift.append(f"generated file drifted: {path}")
    if drift:
        raise CompositionError("\n".join(drift))


def _arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=("generate", "check"))
    parser.add_argument("--repo-root", type=Path, default=Path("."))
    parser.add_argument("--profile", type=Path, required=True)
    parser.add_argument("--components", type=Path, default=Path("components"))
    parser.add_argument("--lock", type=Path, default=Path("Crucible.lock"))
    parser.add_argument(
        "--crate", type=Path, default=Path("crates/crucible-composition")
    )
    return parser.parse_args()


def main() -> int:
    args = _arguments()
    repo_root = args.repo_root.resolve()

    def rooted(path: Path) -> Path:
        return path if path.is_absolute() else repo_root / path

    try:
        resolution = build_resolution(
            repo_root=repo_root,
            profile_path=rooted(args.profile),
            components_root=rooted(args.components),
        )
        if args.mode == "generate":
            generate(
                resolution=resolution,
                lock_path=rooted(args.lock),
                crate_dir=rooted(args.crate),
            )
            print(
                f"composition generated: profile={resolution.profile.name} "
                f"packages={len(resolution.components)} "
                f"sha256={resolution.composition_sha256}"
            )
        else:
            check(
                resolution=resolution,
                lock_path=rooted(args.lock),
                crate_dir=rooted(args.crate),
            )
            print(
                f"composition check: PASS profile={resolution.profile.name} "
                f"sha256={resolution.composition_sha256}"
            )
    except (CompositionError, OSError) as error:
        print(f"composition error: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
