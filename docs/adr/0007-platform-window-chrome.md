# Native window chrome per platform, not one web-style frame

Yard's first shell drew its own title row inside a decorated OS window, so macOS and Windows both showed two chrome layers. The redesign moves the frame into the app: macOS keeps its traffic lights over an overlay title bar, Windows becomes frameless with custom controls, and the shell owns the drag surface. Linux is deferred — it still runs decorated and its chrome is unverified (`docs/bugs.md`).

## Decision

- **macOS:** `titleBarStyle: "Overlay"`, empty title, traffic lights at `{ x: 16, y: 26 }`. The top bar and the sidebar header are drag surfaces. The traffic lights sit over the sidebar header while it is open, and over the top bar's 88px left clearance while it is collapsed.
- **Windows:** `decorations: false` and the default `shadow: true`, which gives an undecorated Windows 11 window rounded corners and a 1px border. The top bar renders minimize / maximize / close on the right. `tauri.windows.conf.json` is merged into the base config — it already carries the bundle engine layout.
- **Linux:** no overlay and no custom window config. The base config's macOS-only fields are ignored; the shell still renders non-macOS controls, which is why the result is unverified and unshipped.
- **Platform detection:** `@tauri-apps/plugin-os` `platform()` is read synchronously before the first render. It is compile-time injected, not IPC, so there is no bootstrap await or first-paint delay. A missing injected global (bare-browser dev) logs once and yields `unknown`, which gets non-macOS chrome. `PlatformProvider` exposes the value; tests supply it directly.
- **Drag and zoom:** a `DragSurface` calls `startDragging()` on primary-button `mousedown` with `detail === 1`, and `toggleMaximize()` on double-click. Caller handlers cannot replace these; interactive children stop propagation on both events.
- **Sidebar:** the shadcn `Sidebar` is vendored as generated apart from two local tweaks — `collapsible="offcanvas"` on desktop and the vendored `Sheet` below 768px. `src/components/ui/sidebar.tsx` keeps: `min-w-0` on `SidebarInset` and a `SidebarTrigger` that labels itself collapse/expand with fold/unfold icons. Desktop open state (`open`) and the mobile branch (`openMobile`) stay in `SidebarProvider`; every "is it showing" check uses `isMobile ? openMobile : open`.
- **macOS menu:** `lib.rs` installs a custom `Yard` / `Edit` / `Window` menu built from predefined items. It has no About item, so the in-app About dialog is the only About surface. Tauri's default menu would add a second one. ⌘Q, ⌘C/⌘V and ⌘W keep working.
- **Quit path:** every window close is intercepted (`onCloseRequested` → `preventDefault`). While compressing, a native confirm decides; confirmed or idle closes go through the `quit` command exactly once. The Windows close button and the ⋯ Quit item call `getCurrentWindow().close()`, never `invoke("quit")` directly. ⌘Q bypasses the handler and exits directly — accepted.
- **Window state and single instance:** `tauri-plugin-window-state` restores maximized, visible and fullscreen, but never size, position or decorations — its physical values shrink or land on the wrong monitor when displays use different scale factors. The app saves logical size and position itself (`window-geometry.json`) and restores them with `LogicalSize`/`LogicalPosition`; a stale state file must not re-decorate the frameless Windows window. `tauri-plugin-single-instance` is registered first and unminimizes, shows and focuses the existing window on a second launch.
- **No transparency:** windows are opaque. `transparent` is only for see-through content, needs `macOSPrivateApi` on macOS and adds composition cost on Windows.

## Known limitation

With the macOS overlay title bar the window cannot be dragged while it is unfocused: the first click focuses it, a second drags ([tauri#4316](https://github.com/tauri-apps/tauri/issues/4316)). The reported issue is macOS-specific; the frameless Windows drag path has not been separately verified.

## Considered Options

- **One decorated window on every platform (previous)** — the in-app title row duplicated the OS chrome and wasted 48px.
- **Frameless everywhere, including Linux** — Linux is not packaged in this slice; shipping untested frameless chrome and controls would be unverified. Keep native decorations until a dedicated Linux pass.
- **Transparent Windows window** — the content is opaque, so it buys nothing and costs composition.
- **Icon-rail sidebar or a forked shadcn Sidebar** — the stock offcanvas branch plus the Sheet already covers 760–900px; a fork would be maintained forever for a behavior the redesign removed.
- **Auto-collapse the sidebar on cramped boards** — it decided from content width and overrode the user; removed with `useAutoCollapse`, boards scroll instead.
- **Tauri's default macOS menu** — includes About, producing two About surfaces.
- **Direct `invoke("quit")` from menu and buttons** — skips the close guard and can drop an in-flight batch without confirmation.

## Consequences

- Window chrome now depends on the platform value; a detection failure degrades to non-macOS chrome rather than a blank frame.
- macOS traffic-light clearance is layout-dependent: the top bar gets 88px left padding only while the sidebar is hidden.
- The top bar height (48px) is both a layout constant and the drag target; buttons must keep stopping propagation.
- Linux needs a deliberate pass (native controls, drag regions, decoration config) before it can be packaged.
