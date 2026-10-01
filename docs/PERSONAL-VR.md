# Personal VR navigator

This checkout uses the local `personal-vr-navigator` branch. It is separate from
the main checkout on the Desktop. No commit or push is needed to build and use it.
The earlier central IBIS information panel is not included.

In VR the existing navigator appears as a small display attached to the bus.
Its placement is measured from the model's standard driver camera and stays fixed
when the bus moves, you look around, change your seat, or recenter the headset.
No bus files are modified; mod and payware buses use the same mechanism.

## Position with the mouse

Press **Ctrl+Shift+M** in VR to start positioning directly. This shortcut is editable
under **Controls → Keyboard → VR: Position navigator** and only works in VR.
Alternatively, open **Esc → Options → VR → Navigator position (this bus)**, then select
**Move and rotate with the mouse...**. The single-player game pauses while editing.
The navigator is selected for the whole edit session: dragging anywhere moves it,
so there is no need to find a small cursor or click exactly on the display.

| Input | Action |
| --- | --- |
| Hold left mouse and move | Move across the current view in 3D |
| Hold right mouse and move | Turn and tilt the display |
| Shift + right mouse drag | Roll the display sideways |
| Mouse wheel | Move closer to or farther from your head |
| Ctrl + mouse wheel | Increase or decrease its physical size |
| R | Restore the starting placement and size |
| Esc or Enter | Save and finish |

Mouse actions do not operate the cockpit while this mode is active. Losing window
focus finishes and saves the edit too. Multiplayer continues running, so position
the display while parked. The options also offer numeric adjustments and opacity.

**Ctrl+Shift+N** toggles visibility and is editable under **Controls → Keyboard**.
The existing **Shift+N** also works: map, map with stop list, off.
Placement, size, rotation, opacity and visibility are saved per vehicle file in
`%USERPROFILE%\.openomsi\vr-navigator.json` (beside the game's settings).

The display is a floating VR overlay with stereo depth, not a replacement for a
physical bus mesh. Place it in an unobstructed space: cockpit geometry does not
occlude it. The map texture updates at its existing 30 Hz; placement and head
tracking are projected every VR frame. Desktop navigation keeps its existing
appearance and settings.

## Build and run

Run in PowerShell:

```powershell
Set-Location "$env:USERPROFILE\.codex\worktrees\personal-vr-navigator\openOmsiCode"
$env:Path = 'C:\Program Files (x86)\Microsoft Visual Studio\18\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin;' + $env:Path
cargo build --release -p omsi-app --bin openomsi --target-dir "$env:USERPROFILE\Desktop\openOmsiPersonalBuild"
if ($LASTEXITCODE -eq 0) { & '.\Start-PersonalVR.cmd' }
```

For subsequent launches, double-click `Start-PersonalVR.cmd` in this checkout.
It disables automatic update checks for this launch, so an official release does
not replace the personal executable. The normal build and its updater are separate.
To get future source updates, merge/rebase the current main into this personal
branch while preserving its local changes, then rebuild it. Do not accept an
official binary update in this personal launcher if you want to keep the feature.
