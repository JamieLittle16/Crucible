#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import urllib.error
import urllib.request
import zipfile
from pathlib import Path

MANIFEST_URL = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json"
TARGET_PROTOCOL = 776
TARGET_DATA_VERSION = 4903

CLASS_NAMES = {
    "blocks": "net.minecraft.world.level.block.Blocks",
    "block": "net.minecraft.world.level.block.Block",
    "block_state": "net.minecraft.world.level.block.state.BlockState",
    "state_base": "net.minecraft.world.level.block.state.BlockBehaviour$BlockStateBase",
    "fluid_state": "net.minecraft.world.level.material.FluidState",
}


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def sha1_file(path: Path) -> str:
    digest = hashlib.sha1()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def fetch_json(url: str) -> dict[str, object]:
    with urllib.request.urlopen(url, timeout=30) as response:
        value = json.load(response)
    if not isinstance(value, dict):
        raise ValueError(f"expected JSON object from {url}")
    return value


def download(
    url: str,
    path: Path,
    expected_sha1: str | None = None,
    expected_size: int | None = None,
) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    if not path.exists():
        with urllib.request.urlopen(url, timeout=60) as response, path.open("wb") as output:
            shutil.copyfileobj(response, output)
    if expected_size is not None and path.stat().st_size != expected_size:
        raise ValueError(f"size mismatch for {path}")
    if expected_sha1 is not None and sha1_file(path) != expected_sha1:
        raise ValueError(f"SHA-1 mismatch for {path}")


def resolve_official_assets(
    version: str,
    cache: Path,
) -> tuple[dict[str, tuple[Path, dict[str, object]]], dict[str, object]]:
    manifest = fetch_json(MANIFEST_URL)
    versions = manifest.get("versions")
    if not isinstance(versions, list):
        raise ValueError("official version manifest has no versions list")
    entry = next(
        (
            item
            for item in versions
            if isinstance(item, dict) and item.get("id") == version
        ),
        None,
    )
    if entry is None:
        raise ValueError(f"official launcher manifest has no version {version}")
    version_url = entry.get("url")
    if not isinstance(version_url, str):
        raise ValueError("official version entry has no metadata URL")
    metadata = fetch_json(version_url)
    downloads = metadata.get("downloads")
    if not isinstance(downloads, dict):
        raise ValueError("official version metadata has no downloads object")

    result: dict[str, tuple[Path, dict[str, object]]] = {}
    for name in ("server", "server_mappings"):
        asset = downloads.get(name)
        if not isinstance(asset, dict):
            raise ValueError(f"official version metadata has no {name} download")
        url = asset.get("url")
        if not isinstance(url, str):
            raise ValueError(f"official {name} download has no URL")
        path = cache / f"{version}-{name}.bin"
        download(
            url,
            path,
            str(asset["sha1"]) if "sha1" in asset else None,
            int(asset["size"]) if "size" in asset else None,
        )
        result[name] = (path, asset)
    return result, metadata


def parse_mappings(
    text: str,
) -> tuple[
    dict[str, str],
    dict[tuple[str, str], str],
    dict[tuple[str, str], set[str]],
]:
    classes: dict[str, str] = {}
    fields: dict[tuple[str, str], str] = {}
    methods: dict[tuple[str, str], set[str]] = {}
    current: str | None = None

    for raw in text.splitlines():
        if not raw.strip():
            continue
        if not raw[0].isspace() and " -> " in raw and raw.rstrip().endswith(":"):
            left, right = raw.rstrip()[:-1].split(" -> ", 1)
            current = left.strip()
            classes[current] = right.strip()
            continue
        if current is None or " -> " not in raw:
            continue

        left, right = raw.strip().rsplit(" -> ", 1)
        left = re.sub(r"^\d+:\d+:", "", left)
        left = re.sub(r":\d+:\d+$", "", left)
        if "(" in left:
            before = left.split("(", 1)[0].strip()
            name = before.split()[-1]
            methods.setdefault((current, name), set()).add(right.strip())
        else:
            parts = left.split()
            if len(parts) >= 2:
                fields[(current, parts[-1])] = right.strip()
    return classes, fields, methods


def unique_method(
    methods: dict[tuple[str, str], set[str]],
    owner: str,
    name: str,
) -> str:
    values = methods.get((owner, name), set())
    if len(values) != 1:
        raise ValueError(
            f"expected unique mapping for {owner}#{name}, got {sorted(values)}"
        )
    return next(iter(values))


def extract_classpath(bundle: Path, root: Path) -> list[Path]:
    root.mkdir(parents=True, exist_ok=True)
    entries: list[Path] = []
    with zipfile.ZipFile(bundle) as archive:
        jars = [
            name
            for name in archive.namelist()
            if name.endswith(".jar")
            and (
                name.startswith("META-INF/versions/")
                or name.startswith("META-INF/libraries/")
            )
        ]
        if not jars:
            return [bundle]
        for index, name in enumerate(jars):
            path = root / f"{index:04d}-{Path(name).name}"
            path.write_bytes(archive.read(name))
            entries.append(path)
    return entries


def render_java_probe(mapping_text: str) -> str:
    classes, fields, methods = parse_mappings(mapping_text)
    obfuscated = {key: classes[value] for key, value in CLASS_NAMES.items()}
    registry_field = fields[(CLASS_NAMES["block"], "BLOCK_STATE_REGISTRY")]
    names = {
        "get_id": unique_method(methods, CLASS_NAMES["block"], "getId"),
        "is_air": unique_method(methods, CLASS_NAMES["state_base"], "isAir"),
        "random_block": unique_method(
            methods,
            CLASS_NAMES["state_base"],
            "isRandomlyTicking",
        ),
        "get_fluid": unique_method(
            methods,
            CLASS_NAMES["state_base"],
            "getFluidState",
        ),
        "fluid_empty": unique_method(
            methods,
            CLASS_NAMES["fluid_state"],
            "isEmpty",
        ),
        "random_fluid": unique_method(
            methods,
            CLASS_NAMES["fluid_state"],
            "isRandomlyTicking",
        ),
    }

    return f'''import java.lang.reflect.*;

public final class CrucibleStateProbe {{
  static Method zero(Class<?> owner, String name) throws Exception {{
    Method method = owner.getMethod(name);
    method.setAccessible(true);
    return method;
  }}

  static Method oneStatic(Class<?> owner, String name) throws Exception {{
    for (Method method : owner.getMethods()) {{
      if (method.getName().equals(name)
          && Modifier.isStatic(method.getModifiers())
          && method.getParameterCount() == 1) {{
        method.setAccessible(true);
        return method;
      }}
    }}
    throw new NoSuchMethodException(name);
  }}

  public static void main(String[] args) throws Exception {{
    Class.forName("{obfuscated['blocks']}", true, CrucibleStateProbe.class.getClassLoader());
    Class<?> block = Class.forName("{obfuscated['block']}");
    Field registryField = block.getField("{registry_field}");
    registryField.setAccessible(true);
    Object registry = registryField.get(null);

    Method getId = oneStatic(block, "{names['get_id']}");
    Class<?> stateBase = Class.forName("{obfuscated['state_base']}");
    Method isAir = zero(stateBase, "{names['is_air']}");
    Method randomBlock = zero(stateBase, "{names['random_block']}");
    Method getFluid = zero(stateBase, "{names['get_fluid']}");
    Class<?> fluidState = Class.forName("{obfuscated['fluid_state']}");
    Method fluidEmpty = zero(fluidState, "{names['fluid_empty']}");
    Method randomFluid = zero(fluidState, "{names['random_fluid']}");

    for (Object state : (Iterable<?>) registry) {{
      int id = ((Number) getId.invoke(null, state)).intValue();
      boolean air = (Boolean) isAir.invoke(state);
      boolean blockRandom = !air && (Boolean) randomBlock.invoke(state);
      Object fluid = getFluid.invoke(state);
      boolean countedFluid = !air && !(Boolean) fluidEmpty.invoke(fluid);
      boolean fluidRandom = countedFluid && (Boolean) randomFluid.invoke(fluid);
      String raw = state.toString().replace("\\t", " ").replace("\\n", " ");
      System.out.println(
          id + "\\t" + (air ? 0 : 1)
              + "\\t" + (countedFluid ? 1 : 0)
              + "\\t" + (blockRandom ? 1 : 0)
              + "\\t" + (fluidRandom ? 1 : 0)
              + "\\t" + raw);
    }}
  }}
}}
'''


def canonical_key(raw: str) -> str:
    match = re.fullmatch(r"Block\{([^}]+)\}(?:\[(.*)\])?", raw)
    if match is None:
        raise ValueError(f"unexpected BlockState string: {raw}")
    block = match.group(1)
    properties = match.group(2)
    if not properties:
        return block
    parts = sorted(part for part in properties.split(",") if part)
    return block + "[" + ",".join(parts) + "]"


def run_probe(server: Path, mappings: Path, work: Path) -> list[dict[str, object]]:
    classpath_entries = extract_classpath(server, work / "classpath")
    mapping_text = mappings.read_text(encoding="utf-8")
    java_path = work / "CrucibleStateProbe.java"
    java_path.write_text(render_java_probe(mapping_text), encoding="utf-8")
    classpath = os.pathsep.join(str(path) for path in classpath_entries)

    subprocess.run(
        ["javac", "-encoding", "UTF-8", "-d", str(work), str(java_path)],
        check=True,
    )
    result = subprocess.run(
        ["java", "-cp", str(work) + os.pathsep + classpath, "CrucibleStateProbe"],
        check=True,
        text=True,
        capture_output=True,
    )

    states: list[dict[str, object]] = []
    for line in result.stdout.splitlines():
        if not line.strip():
            continue
        parts = line.split("\t", 5)
        if len(parts) != 6:
            raise ValueError(f"invalid probe line: {line}")
        vanilla_id, non_air, counted_fluid, random_block, random_fluid, raw = parts
        states.append(
            {
                "key": canonical_key(raw),
                "vanilla_id": int(vanilla_id),
                "non_air": non_air == "1",
                "counted_fluid": counted_fluid == "1",
                "random_block": random_block == "1",
                "random_fluid": random_fluid == "1",
            }
        )

    states.sort(key=lambda state: int(state["vanilla_id"]))
    ids = [int(state["vanilla_id"]) for state in states]
    if ids != list(range(len(states))):
        raise ValueError("official BLOCK_STATE_REGISTRY is not dense in probe output")
    return states


def extract(
    version: str,
    output: Path,
    cache: Path,
    server: Path | None,
    mappings: Path | None,
) -> None:
    if server is None or mappings is None:
        resolved, _ = resolve_official_assets(version, cache)
        server = server or resolved["server"][0]
        mappings = mappings or resolved["server_mappings"][0]

    with tempfile.TemporaryDirectory(prefix="crucible-state-probe-") as directory:
        states = run_probe(server, mappings, Path(directory))

    data = {
        "schema": 1,
        "target": {
            "minecraft_version": version,
            "protocol_version": TARGET_PROTOCOL,
            "data_version": TARGET_DATA_VERSION,
        },
        "air_key": "minecraft:air",
        "provenance": {
            "server_sha256": sha256_file(server),
            "server_mappings_sha256": sha256_file(mappings),
            "source": "official-runtime-reflection-probe-v1",
        },
        "states": states,
    }
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(
        json.dumps(data, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(f"extracted {len(states)} official block states -> {output}")


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Extract section-relevant block-state facts from the official server"
    )
    parser.add_argument("--version", default="26.2")
    parser.add_argument("--output", required=True)
    parser.add_argument("--cache", default=".crucible/vanilla/downloads")
    parser.add_argument("--server-jar")
    parser.add_argument("--server-mappings")
    args = parser.parse_args()

    try:
        extract(
            args.version,
            Path(args.output),
            Path(args.cache),
            Path(args.server_jar) if args.server_jar else None,
            Path(args.server_mappings) if args.server_mappings else None,
        )
        return 0
    except (
        KeyError,
        OSError,
        ValueError,
        subprocess.CalledProcessError,
        urllib.error.URLError,
    ) as error:
        print(f"error: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
