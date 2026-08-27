# Security Policy

Helve is under active development. Security reports are welcome, but the project does not yet claim a stable production release surface.

## Reporting a vulnerability

Do **not** open a public issue for a vulnerability that could enable remote code execution, authentication/session compromise, denial of service at materially lower cost than ordinary gameplay traffic, world/save corruption, privilege escalation, secret disclosure, or another issue whose details would materially help exploitation.

Use GitHub **Private vulnerability reporting** for this repository instead. The repository must keep that feature enabled while public.

A useful report includes:

- affected commit/version and configuration;
- the security boundary that is crossed;
- minimal reproduction steps or a proof of concept when safe to provide;
- expected versus observed behavior;
- realistic impact and attacker prerequisites;
- any suggested regression test or containment boundary.

Please do not include Mojang source code, proprietary server artifacts, private credentials, player data, or third-party secrets in a report. Describe or hash proprietary evidence instead where possible.

## Supported versions

Until Helve has formal releases, only the current development line is actively maintained. Historical commits and experimental branches do not receive security backports.

## Disclosure

Please allow a reasonable opportunity to reproduce, fix, qualify, and publish a security update before public disclosure. Security fixes remain subject to Helve's normal correctness and regression requirements; urgency does not justify knowingly weakening another safety boundary.
