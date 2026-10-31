---
id: settings-diagrams
title: Diagrams and shell integration
summary: The Diagrams section — how diagrams are checked and drawn — and, on Windows, the Shell integration section.
keywords: [diagrams, archify, quality profile, palette, motion, reduced motion, shell integration, explorer, right-click, open in kubiq, windows]
order: 110
status: current
related: [settings, settings-appearance]
---

**Diagrams** sets how diagram files are checked and drawn, for every diagram. These settings are kept
for you, not in any project. **Shell integration** is a separate section that exists on Windows only.

## The parts

| Part | What it changes for you |
|---|---|
| **Quality profile** | One strictness for the layout checks of every diagram: **Advisory**, **Standard** or **Showcase**; **Document** follows what each file asks for |
| **Palette** | The look of every diagram: **Document** follows each file, otherwise **Classic**, **Signal flow**, **Blueprint** or **Editorial**; **Classic** keeps Kubiq's colours, the others restyle the whole picture |
| **Motion** | Turned off, the view jumps to a node instead of gliding and fades are instant; your system's reduced-motion setting does the same |
| **Open in Kubiq** | Windows only, under **Shell integration**: adds *Open in Kubiq* to the right-click menu of a file, a folder, or the empty space in an open folder, for your user only |
| **Install**, **Update**, **Remove** | The one button beside **Open in Kubiq**, naming what it will do; **Update** appears when the entry launches another build of Kubiq, and **Remove** deletes only what Kubiq added |

## What you can do here

- Make diagrams hold still: turn **Motion** off. The window's own theme is in
  [Appearance and size](settings-appearance).

## When something goes wrong

- **A diagram looks the same after I changed Palette.** The file sets its own look and **Palette**
  is on **Document**; pick a named palette to override it.
- **The Shell integration section is not there.** It is shown on Windows only.
- **It says it has no file-manager menu Kubiq can join.** The machine Kubiq runs on cannot take the
  entry; nothing is changed.
- **The status reads Installed, launches another build of Kubiq.** Press **Update** so the menu opens
  this build.
