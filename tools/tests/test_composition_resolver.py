from __future__ import annotations

import tempfile
import textwrap
import unittest
from pathlib import Path

from tools import composition_resolver as resolver


PROFILE = """
minecraft = "26.2"

[profile]
name = "reference"
fidelity = "strict"
optimize = "debuggability"

[engine]
"world.section-store" = "section-reference"

[security]
allow_unqualified = false
allow_third_party_native = false
allow_semantic_deviations = false
""".lstrip()


def component_text(
    *,
    package_id: str = "section-reference",
    capability: str = "world.section-store/1",
    kind: str = "engine-source",
    trust: str = "engine-native",
    fidelity: str = "strict",
    qualified: bool = True,
    deviations: tuple[str, ...] = (),
    requires: tuple[str, ...] = (),
) -> str:
    deviation_text = ", ".join(f'"{item}"' for item in deviations)
    require_text = "".join(
        f'\n[[requires]]\ncapability = "{capability_id}"\n'
        for capability_id in requires
    )
    return textwrap.dedent(
        f"""
        schema = 1

        [package]
        id = "{package_id}"
        version = "0.0.0"
        kind = "{kind}"
        trust = "{trust}"
        fidelity = "{fidelity}"
        qualified = {str(qualified).lower()}
        crate = "fixture-crate"
        crate_path = "crates/fixture-crate"
        rust_crate = "fixture_crate"
        semantic_deviations = [{deviation_text}]
        qualification_records = ["docs/qualification.md"]

        [compatibility]
        minecraft = ["26.2"]

        [[provides]]
        capability = "{capability}"
        cardinality = "exactly-one"
        cost = "hot"
        rust_export = "SectionStore"
        rust_type = "fixture_crate::SectionStore"
        {require_text}
        """
    ).lstrip()


class FixtureRepo:
    def __init__(self, root: Path) -> None:
        self.root = root
        (root / "profiles").mkdir(parents=True)
        (root / "components").mkdir()
        (root / "crates/fixture-crate").mkdir(parents=True)
        (root / "docs").mkdir()
        (root / "vanilla/state-data").mkdir(parents=True)
        (root / "crates/fixture-crate/Cargo.toml").write_text(
            "[package]\nname = \"fixture-crate\"\nversion = \"0.0.0\"\n",
            encoding="utf-8",
        )
        (root / "docs/qualification.md").write_text("qualified\n", encoding="utf-8")
        (root / "rust-toolchain.toml").write_text(
            '[toolchain]\nchannel = "1.97.1"\n', encoding="utf-8"
        )
        (root / "vanilla/state-data/26.2-state-data-manifest.json").write_text(
            """{
  "generation_digest": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  "target": {"minecraft_version": "26.2"}
}\n""",
            encoding="utf-8",
        )
        self.profile = root / "profiles/reference.toml"
        self.profile.write_text(PROFILE, encoding="utf-8")

    def add_component(self, package_id: str, text: str | None = None) -> Path:
        directory = self.root / "components" / package_id
        directory.mkdir(parents=True)
        path = directory / "component.toml"
        path.write_text(
            component_text(package_id=package_id) if text is None else text,
            encoding="utf-8",
        )
        return path

    def resolution(self) -> resolver.Resolution:
        return resolver.build_resolution(
            repo_root=self.root,
            profile_path=self.profile,
            components_root=self.root / "components",
        )


class CompositionResolverTests(unittest.TestCase):
    def test_reference_profile_resolves_to_concrete_static_provider(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = FixtureRepo(Path(raw))
            fixture.add_component("section-reference")
            resolution = fixture.resolution()

            self.assertEqual(resolution.profile.name, "reference")
            self.assertEqual(len(resolution.components), 1)
            self.assertEqual(resolution.components[0].package_id, "section-reference")
            self.assertEqual(len(resolution.providers), 1)
            capability, package_id, export, rust_type, cost = resolution.providers[0]
            self.assertEqual(capability.identity, "world.section-store/1")
            self.assertEqual(package_id, "section-reference")
            self.assertEqual(export, "SectionStore")
            self.assertEqual(rust_type, "fixture_crate::SectionStore")
            self.assertEqual(cost, "hot")

            generated = resolver.render_lib_rs(resolution)
            self.assertIn(
                "pub use fixture_crate::SectionStore as SectionStore;", generated
            )
            executable = "\n".join(
                line
                for line in generated.splitlines()
                if not line.lstrip().startswith("//")
            )
            self.assertNotIn("dyn ", executable)
            self.assertNotIn("HashMap", executable)
            self.assertNotIn("service", executable.lower())

    def test_capability_requires_explicit_positive_version(self) -> None:
        for capability in ("world.section-store", "world.section-store/0", "bad//1"):
            with self.subTest(capability=capability), tempfile.TemporaryDirectory() as raw:
                fixture = FixtureRepo(Path(raw))
                fixture.add_component(
                    "section-reference", component_text(capability=capability)
                )
                with self.assertRaises(resolver.CompositionError):
                    fixture.resolution()

    def test_unresolved_profile_is_intent_only_not_runnable(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = FixtureRepo(Path(raw))
            fixture.add_component("section-reference")
            unresolved = PROFILE.replace(
                '[engine]\n"world.section-store" = "section-reference"\n\n', ""
            )
            fixture.profile.write_text(unresolved, encoding="utf-8")
            with self.assertRaisesRegex(resolver.CompositionError, "no engine selections"):
                fixture.resolution()

    def test_profile_unknown_field_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = FixtureRepo(Path(raw))
            fixture.add_component("section-reference")
            fixture.profile.write_text(PROFILE + "\nunknown = true\n", encoding="utf-8")
            with self.assertRaisesRegex(resolver.CompositionError, "unknown keys"):
                fixture.resolution()

    def test_component_unknown_field_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = FixtureRepo(Path(raw))
            text = component_text().replace(
                'qualified = true\n', 'qualified = true\nsecret_mode = true\n'
            )
            fixture.add_component("section-reference", text)
            with self.assertRaisesRegex(resolver.CompositionError, "unknown keys"):
                fixture.resolution()

    def test_strict_profile_rejects_relaxed_component(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = FixtureRepo(Path(raw))
            fixture.add_component(
                "section-reference", component_text(fidelity="relaxed")
            )
            with self.assertRaisesRegex(resolver.CompositionError, "strict profile"):
                fixture.resolution()

    def test_profile_rejects_unqualified_component(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = FixtureRepo(Path(raw))
            fixture.add_component(
                "section-reference", component_text(qualified=False)
            )
            with self.assertRaisesRegex(resolver.CompositionError, "unqualified"):
                fixture.resolution()

    def test_profile_rejects_semantic_deviation(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = FixtureRepo(Path(raw))
            fixture.add_component(
                "section-reference",
                component_text(deviations=("SEM-world.section-store-001",)),
            )
            with self.assertRaisesRegex(resolver.CompositionError, "semantic deviations"):
                fixture.resolution()

    def test_profile_rejects_third_party_native_component(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = FixtureRepo(Path(raw))
            fixture.add_component(
                "section-reference",
                component_text(kind="native-module", trust="trusted-native"),
            )
            with self.assertRaisesRegex(resolver.CompositionError, "third-party native"):
                fixture.resolution()

    def test_qualified_component_requires_existing_qualification_record(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = FixtureRepo(Path(raw))
            fixture.add_component("section-reference")
            (fixture.root / "docs/qualification.md").unlink()
            with self.assertRaisesRegex(resolver.CompositionError, "qualification record"):
                fixture.resolution()

    def test_component_crate_path_cannot_escape_repository(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = FixtureRepo(Path(raw))
            text = component_text().replace(
                'crate_path = "crates/fixture-crate"', 'crate_path = "../outside"'
            )
            fixture.add_component("section-reference", text)
            with self.assertRaisesRegex(resolver.CompositionError, "without '..'"):
                fixture.resolution()

    def test_requirement_must_resolve_uniquely(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = FixtureRepo(Path(raw))
            fixture.add_component(
                "section-reference",
                component_text(requires=("world.clock/1",)),
            )
            clock_a = component_text(
                package_id="clock-a", capability="world.clock/1"
            ).replace("SectionStore", "Clock")
            clock_b = component_text(
                package_id="clock-b", capability="world.clock/1"
            ).replace("SectionStore", "Clock")
            fixture.add_component("clock-a", clock_a)
            fixture.add_component("clock-b", clock_b)
            with self.assertRaisesRegex(resolver.CompositionError, "does not resolve uniquely"):
                fixture.resolution()

    def test_dependency_closure_resolves_unique_provider(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = FixtureRepo(Path(raw))
            fixture.add_component(
                "section-reference",
                component_text(requires=("world.clock/1",)),
            )
            clock = component_text(
                package_id="clock", capability="world.clock/1"
            ).replace("SectionStore", "Clock")
            fixture.add_component("clock", clock)
            resolution = fixture.resolution()
            self.assertEqual(
                {component.package_id for component in resolution.components},
                {"section-reference", "clock"},
            )

    def test_generated_files_are_deterministic_and_drift_is_detected(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = FixtureRepo(Path(raw))
            fixture.add_component("section-reference")
            resolution = fixture.resolution()
            lock = fixture.root / "Crucible.lock"
            crate = fixture.root / "crates/crucible-composition"

            resolver.generate(resolution=resolution, lock_path=lock, crate_dir=crate)
            first_lock = lock.read_bytes()
            first_toml = (crate / "Cargo.toml").read_bytes()
            first_lib = (crate / "src/lib.rs").read_bytes()

            resolver.check(resolution=resolution, lock_path=lock, crate_dir=crate)
            resolver.generate(resolution=resolution, lock_path=lock, crate_dir=crate)
            self.assertEqual(lock.read_bytes(), first_lock)
            self.assertEqual((crate / "Cargo.toml").read_bytes(), first_toml)
            self.assertEqual((crate / "src/lib.rs").read_bytes(), first_lib)

            (crate / "src/lib.rs").write_text("drift\n", encoding="utf-8")
            with self.assertRaisesRegex(resolver.CompositionError, "drifted"):
                resolver.check(resolution=resolution, lock_path=lock, crate_dir=crate)

    def test_manifest_bytes_change_composition_identity(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = FixtureRepo(Path(raw))
            manifest = fixture.add_component("section-reference")
            first = fixture.resolution().composition_sha256
            manifest.write_text(
                manifest.read_text(encoding="utf-8") + "\n# provenance-only edit\n",
                encoding="utf-8",
            )
            second = fixture.resolution().composition_sha256
            self.assertNotEqual(first, second)

    def test_profile_bytes_change_composition_identity(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = FixtureRepo(Path(raw))
            fixture.add_component("section-reference")
            first = fixture.resolution().composition_sha256
            fixture.profile.write_text(
                fixture.profile.read_text(encoding="utf-8") + "\n# operator note\n",
                encoding="utf-8",
            )
            second = fixture.resolution().composition_sha256
            self.assertNotEqual(first, second)


if __name__ == "__main__":
    unittest.main()
