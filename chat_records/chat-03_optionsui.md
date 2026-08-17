# Next, could you add a setting button that would...

Exported: 8/17/2026, 10:27:01 PM

---

**User:**

Next, could you add a setting button that would use a cog as icon, which would allow users to select saved video path, max recording time by second (if the time is 0, then the max record time would be unlimited), as well as hotkey to trigger recording/stop recording?
The config would be stored to %appdata%\cms_video_recorder\config.json.

---

**Assistant:**

I'll start by understanding the existing codebase structure.

Now I'll implement. Starting with dependencies:

Now the config module:

Now the global hotkey listener:

Now the folder picker dialog:

Now the settings panel state, including the cog icon:

Now the toolbar. First the imports, messages, and state:

Good — internal cfg blocks. Let me restructure the hotkey and dialog modules to match:

Now the toolbar state and update logic:

Now the update arms:

Now the new update arms and helpers:

Now add the settings helpers next to the recording helpers:

Now the view: body layout, settings button, and the panel.

Now the settings panel view:

Now the small building blocks and styles:

Now wire up the new modules in main.rs:

---

**User:**

The cog is showing correctly in settings. Could you display assigned hotkey in options UI?

---

**Assistant:**

The cog renders well. Let me clean up the debug scaffolding first, then improve the hotkey display.

Debug scaffolding removed. Now the hotkey display — let me look at the current row:

Now the panel row, rebuilt as key caps with separate controls:

Now the display helper and the key-cap widget:

Now the `ClearHotkey` message and the panel height:

---
