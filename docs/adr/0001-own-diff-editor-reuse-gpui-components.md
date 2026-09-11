---
status: accepted
---

# Own the diff editor and reuse GPUI components

yori will implement a specialized editable diff surface on public native GPUI
APIs, while using GPUI Kit / Component as ordinary dependencies for standard
controls and consistent dark theming. We choose project-owned editor behavior
rather than vendoring, extracting or patching an upstream editor, because source
alignment and review/edit/restore interactions define this product and the
examined reusable editors do not expose the needed display projection.

## Boundaries and trade-offs

- Reuse tabs, menus, buttons, dialogs, tooltips and split panes; do not build a
  replacement component toolkit. The custom editor and controls must use the
  same compatible GPUI package family.
- Reuse text-buffer, diff and syntax libraries where useful. GPUI supplies text
  shaping and platform input interfaces, not complete editor semantics. yori
  owns selection, input integration, undo, source/display mapping, diff gutters,
  alignment and change-transfer interaction.
- Alignment rows are presentation only. They are never source text, selectable
  phantom bytes or fake line numbers.
- Reviewing and editing use one surface. A comparison can pair a read-only
  baseline with an editable local file, while other roles may use different
  editability rules. Restoring a baseline block is an undoable edit, not
  three-way conflict resolution.
- Keep native GPUI. Other native toolkits, webview editors, and extracting a
  larger application's editor were rejected because they either changed the
  product stack or transferred an unsuitable maintenance boundary into yori.
- This costs more initial editor work than adaptation. A narrow product scope
  excludes IDE features, not Unicode, native input composition, clipboard,
  undo or correct selection behavior. Performance remains to be measured.

This decision establishes the ownership boundary rather than a fixed internal
architecture. Buffer libraries, interfaces, dependency versions, and crate seams
may continue to evolve without reopening it.
