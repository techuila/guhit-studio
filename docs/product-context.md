# Product context

Condensed from the confidential market research working paper (21 Sep 2026), which is not in the repo.

## Thesis
Do not recreate AutoCAD. Build the fastest path from architectural intent to an editable, dimension-aware plan, a synchronized 3D model and a client-ready visual. Win the workflow, not the command list.

## Users
| Segment | Priority |
|---|---|
| Solo architect, freelancer | P0 |
| Small firm (2 to 20) | P0 |
| Architecture student, young professional | P0 acquisition |
| Design-build contractor, interior designer | P1 |

Launch domain: Philippine residential, mostly single houses.

## MVP promise
"Draw a real floor plan in minutes, see it in 3D instantly, ask AI to make revisions, and generate a client-ready visual, in one workspace."

## Scope map
| Priority | Feature | Status in this build |
|---|---|---|
| P0 | Projects, autosave, thumbnails | local files |
| P0 | 2D canvas, walls, doors/windows, rooms + area, dimensions/text | in scope |
| P0 | Live 3D, materials, camera presets | in scope |
| P0 | PDF/image export | in scope |
| P0 | AI render | Tier 1 capture in scope, Tier 2 provider open (DECISIONS D9) |
| P1 | AI command copilot, DXF export, plan image underlay, local material presets, Taglish | in scope |
| P1 | Share/review link | out: needs a backend (DECISIONS D1) |
| P2 | Sheets, sections, IFC, multiuser | out |

## UX rules
Simple first, precision always available: progressive disclosure, direct manipulation, type-to-precise, contextual inspector, command palette, AI as another input mode, immediate 3D.

Screen: top bar, left tool rail, canvas (2D, 3D or split), right inspector, AI dock, bottom status bar.

## Trust rules
- AI proposes typed commands, shows a preview, commits only on approval, one undo reverts it.
- Model answers come from model data.
- AI imagery is labelled and never edits geometry.
- No compliance, permit or structural claims.

## Brand
GUHIT Studio. "Draw. See. Build the idea." Off-white canvas, deep blueprint navy, restrained teal, thin geometric linework. Architecture first, no sparkle-AI look.
