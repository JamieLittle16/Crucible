Experimental Crucible milestone snapshot. This is not a production or playable server release.

The R2A milestone demonstrated that an unmodified Minecraft: Java Edition 26.2 client can complete Crucible's existing pre-Play route, cross from the finite R1X visible-world bootstrap into Crucible-owned live Play liveness, and remain connected across repeated source-compatible keep-alive transactions. The qualified stock-client run acknowledged ten consecutive Crucible-generated keep-alive challenges and ended by normal peer EOF rather than timeout or invalid-response rejection.

The visible-world bootstrap remains explicitly experimental and replay-backed, and permanent Play packet admission is still incomplete at this milestone. `production_admitted=false` remains intentional.

This release asset contains the `crucible-server` executable, MPL-2.0 licence, and repository README for the tagged source revision.
