<div align="center">

<!-- Banner / Logo Placeholder -->
<p align="center">
  <span style="font-size: 72px;">📱🕹️💻</span>
  <!-- <img src="assets/banner.png" alt="AgentPad Banner" width="720" /> -->
</p>

# AgentPad

**Hands off the keyboard and mouse. Pick up your phone as the ultimate command surface — lean back, kick your feet up, or relax on the sofa while your AI Agent works at full throttle!**

<p align="center">
  <b>English</b> | <a href="README.zh-CN.md">简体中文</a>
</p>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-AGPL--3.0-blue.svg" alt="License: AGPL-3.0" /></a>
  <img src="https://img.shields.io/badge/Platform-Windows%20%7C%20macOS%20%7C%20Android-blueviolet" alt="Platform: Windows | macOS | Android" />
  <img src="https://img.shields.io/badge/Network-LAN%20Only-green" alt="Network: LAN Only" />
  <img src="https://img.shields.io/badge/Account-None-lightgrey" alt="Account: None" />
  <img src="https://img.shields.io/badge/Spine%20Health-100%25%20Saved-brightgreen" alt="Spine Health" />
</p>

</div>

> **"Wait! Are you still hunching over your desk like a medieval monk, glued to your monitor for ten hours straight?!"**  
> Stiff neck? Sore shoulders? Lumbar muscle strain? That herniated disc sounding alarm bells?  
> We're in the AI era — why on earth are you still sacrificing your spine just to hit Enter on an Agent confirmation every five minutes?!  
> **Stop! You don't have to suffer like this!**  
> Introducing **AgentPad** — the dedicated remote "walkie-talkie" built for desktop AI Agents!
>
> *" 🕺 Better use AgentPad! "*

---

> **Notice**:  
> AgentPad operates strictly over your local network — it is not a remote desktop, and zero data goes through any cloud relay.  
> Please keep your computer unlocked and place the cursor in the field where you want input to land.  
> macOS requires Accessibility permissions (otherwise system keystroke and cursor injection cannot function).  
> The desktop daemon natively supports Windows and macOS; the mobile client is Android.

---

## Why AgentPad? — Stop the Desk Strain, Command from Comfort!

You bought the most expensive ergonomic chair, yet you're still stuck leaning forward, hunching over your keyboard all day?

With AgentPad, you can stay ten feet away from your desk:

- **Lean all the way back in your ergonomic chair**, or kick your feet up on the desk;
- **Or collapse flat on the sofa across the room**;
- Step away from the screen, ditch the keyboard and mouse, and hold just your phone to command any Agent on your computer like a walkie-talkie!

It doesn't just supercharge your prompt input throughput — it actually saves your spine, neck, and posture from daily desk fatigue!

And it doesn't stop at commanding Agents.  
While your Agent is grinding through code or running deep research in the background and you want to chill with some videos? **You don't even have to get up!**  
Just slide your thumb over the **trackpad, trackball, or pointing stick** at the bottom of your phone screen to switch windows, pop open your browser, jump to YouTube or your favorite streaming site, and voice-search whatever you want to watch — **all handled effortlessly while lying flat on the couch!**

---

## Core Features

AgentPad is a hands-off input solution built for the Agent era — a dedicated "remote control + walkie-talkie" for your desktop.  
Voice is just one pathway, not the whole story.  
Typing, custom shortcut keys, and pointer controls are all fully functional without ever touching your computer.  
AI Agents are the prime use case, but it works just as seamlessly for researching in a browser, replying to chat messages, or operating any window that takes keyboard and mouse input.

#### Text Input

- **Command from across the room** — Sit comfortably on the other side of the room. Speech, typing, tapping shortcuts, and moving the cursor all happen on your phone. When an Agent asks "Which option do you want?", you never have to scramble back to your desk to hit a key.
- **Voice typing, walkie-talkie style** — Dictate using Gboard or your favorite voice-enabled Android keyboard. Transcribed text automatically lands at the desktop cursor once speech stabilizes. Stability delay is configurable: instant (no delay), 0.5s, 1s, or 1.5s (0.5s default). This pause gives voice keyboards with AI formatting and punctuation restructuring enough time to finalize and polish the transcript. You can also turn off auto-send to review and edit before sending.
- **Dedicated send button** — True send control — you decide when the buffer goes out. Auto-send can stay enabled for an instant walkie-talkie flow, or stay disabled when you want to refine drafts first.
- **Typing is a first-class citizen** — Skip the microphone whenever you prefer. Draft and refine on your phone screen, then hit send. Especially useful in noisy environments, when prompt phrasing must be exact, or when your eyes are already on the phone.
- **No desktop IME re-composition** — Text finishes composition on your mobile keyboard. Once placed on the desktop clipboard, it is pasted at the caret using native system shortcuts (`Cmd+V` or `Ctrl+V`). The desktop OS receives a clean paste command, not a sequence of simulated keystrokes that might trigger IME candidates or unwanted autocompletion.
- **Shortcut keys right under your thumb** — When an Agent stops and waits for Esc, Enter, or Shift+Enter, tap the row below the input box. Key bindings can be customized and expanded freely without being locked into any specific agent tool.
- **Genuine system key injection** — Shortcuts inject real keydown and keyup events into the operating system, rather than pasting dummy characters into the box. Esc actually cancels, Enter actually confirms, and Shift+Enter triggers whatever the target application binds it to.
- **Optional automatic Enter** — After text lands, AgentPad can automatically press Enter so the Agent gets straight to work. After a successful paste, it waits 80ms to let the focused application consume the paste before firing Enter; normal text sync and manual Enter shortcuts are not delayed. If the Agent is asking you to pick an option rather than submit, simply toggle this switch off.
- **Undo last input** — Sent the wrong prompt? Tap to send `Ctrl+Z` / `Cmd+Z` to the desktop to undo in standard text fields (terminals and command lines may not support this undo depending on host configuration).

#### Cursor & Pointer Controls

- **Trackpad** — Single-finger move and tap to click, hold and move to drag, hold with slight movement or two-finger tap for right-click, and two-finger vertical swipe to scroll.
- **Trackball** — Roll the virtual glossy red ball for smooth relative pointer movement, with integrated left/right buttons and scroll wheel on the same compact base.
- **Pointing Stick** — Press and nudge the center directional nub for continuous relative motion. Buttons, scroll wheel, and base footprint match the trackball layout, ensuring zero layout shift when switching modes.
- **Reversible scroll wheel side** — Switch the scroll wheel to the left or right side in settings; trackpad, trackball, and pointing stick adapt together.
- **Configurable pointer & wheel speeds** — Tuned via sliders in settings. Pointer speed offers 4 tiers: ×1 / ×2 / ×3 / ×4 (default ×2); wheel speed offers 7 tiers: ×4 / ×8 / ×12 / ×16 / ×20 / ×24 / ×28 (default ×16), clearly displaying physical multipliers.
- **Trackpad height tiers** — Small, Medium, and Large presets adjust trackpad height only; trackball and pointing stick retain their intrinsic compact height.

#### Additional Capabilities

- **Multitask while Agents work** — Want to pass the time while the Agent crunches through tasks? Use the virtual trackpad, trackball, or pointing stick to switch over to your browser, open YouTube or streaming sites, and use voice input to search videos — all without getting off the couch.
- **NIC switching under the QR code** — Computers often run Ethernet and Wi-Fi simultaneously. If the pairing QR advertises the wired IP while your phone is on Wi-Fi, pairing will fail even though the wireless card can reach the phone. Simply switch to the reachable Wi-Fi IP under the QR and scan.
- **Desktop listener on all interfaces** — The server listens on `0.0.0.0:9618` rather than binding only the primary adapter. Any IP selected on the pairing screen can accept connections.
- **Scan QR or enter address manually** — QR scanning is just one gateway. The pairing window always provides a copyable `IP:port`, allowing manual entry without camera access.
- **Manage multiple computers from one phone** — Keep laptops and desktops connected at the same time. Scanning another machine appends it to your device list without kicking existing connections. Toggle active targets below the input.
- **Rename, edit IP, and manual addition** — In the Connected panel, rename devices, update IPs, refresh connections, or tap `+` to add manually. Long-press to reorder or delete.
- **Local LAN only, zero accounts** — Complete privacy: no registration, no login, no cloud relays.
- **Auto-reconnect on screen wake** — Waking your phone from sleep automatically re-polls the saved address pool to restore the link.
- **Friendly with Remote Desktop and Virtual LANs** — Accessing a remote desktop from outside, or connecting phone and PC via ZeroTier, Tailscale, or similar virtual LANs? As long as both devices can unicast to each other, simply select the virtual network IP in the pairing window and scan or paste. No need to dig through system network settings or run terminal commands.
- **Saved candidate address pool** — Optionally save multiple candidate IPs per machine (Wi-Fi, Ethernet, secondary adapters). When switching networks, the phone retries the pool automatically without relying on unreliable UDP broadcasts.

---

## Quick Start (3 Simple Steps)

### 1. Download

Grab the build for your machine from this repository's **Releases**:

| Platform | Installer / Package | Requirements |
| :--- | :--- | :--- |
| **Windows** | `agentpad-windows-x64.exe` | Windows 10 / 11 (64-bit) |
| **macOS** | `agentpad-macos-arm64.dmg` | Apple Silicon (M1 or later) |
| **Android** | `agentpad.apk` | Android 5.0 or later |

---

### 2. Connect

```text
Launch desktop app → Ensure phone & PC are on the same LAN (or hotspot/VLAN) → Pairing code pops up
→ Confirm and select the subnet IP matching your phone under the QR → Scan QR or paste "IP:port" manually
```

- **Windows Users**: Run `agentpad-windows-x64.exe`. Allow TCP **9618** through the firewall if prompted. If the PC uses Ethernet while the phone is on Wi-Fi, switch to the wireless adapter IP under the QR code.
- **macOS Users**: Open the `.dmg`, drag the app into Applications, and right-click → Open on first launch. Grant Accessibility permissions when prompted (essential for key and cursor injection).
- **Android Users**: Install the APK, open the "Connected" panel below the input, tap the scan icon or tap `+` to paste the copied desktop address.

> When multiple devices remain connected, manage, reorder, or delete them in the "Connected" panel.

---

### 3. Start Using

1. **Focus the Caret**: Click where you want text to land on your computer (AI Agent input, chat window, browser search bar, IDE, etc.) using your mouse or phone trackpad.
2. **Input Freely**: Pick up your phone, type in the text box, or activate voice typing (e.g. Gboard). Text can auto-send when you pause, or you can tap the **Send** button manually.
3. **Respond to Prompts**: When the Agent pauses for your choice, tap Esc / Enter / Shift+Enter (or your custom shortcuts) right below the input box.
4. **Remote Control**: Switch apps or navigate the cursor using the trackpad, trackball, or pointing stick at the bottom.

> **Tip**: While the Agent is asking for multiple-choice input, feel free to toggle off **Automatic Enter**; turn it back on when you want every spoken prompt submitted immediately.

---

## FAQ

**Q: Why won't my phone connect to my computer?**  
**A:** First confirm that your phone and PC are on the same local network, phone hotspot, or virtual LAN (such as ZeroTier / Tailscale) where both sides can unicast, and that your computer's firewall allows TCP 9618.  
Under the pairing QR code on your desktop, verify and switch to the network adapter IP that your phone can actually reach (Wi-Fi, Ethernet, or virtual adapter), then scan again or paste that IP.  
macOS users: make sure AgentPad is granted "Accessibility" permissions under System Settings.

**Q: Why did injected text or mouse clicks land in the wrong application?**  
**A:** Desktop injection strictly targets the currently active system focus. Make sure you click and activate the target input box or window before sending.

**Q: Are there any text pasting limitations?**  
**A:** AgentPad delivers plain text.  
Certain protected input boxes (such as secure password fields) or software that disables/remaps system paste shortcuts may reject input.  
macOS retains the sent text on the system clipboard; Windows restores the previous text clipboard content shortly after pasting (non-text items like images are not restored).

**Q: Does voice auto-send work with all third-party keyboards?**  
**A:** AgentPad never guesses input sources from text length, typing speed, or intermediate composition states. Instead, it requires strict source evidence: an Android recording session starting after the input box gains focus, or an IME explicitly declaring `voice` mode (such as Gboard voice typing).  
AgentPad only detects system recording status flags — it never captures or reads actual audio data, and requires no microphone permissions from the user.  
Standard keyboard typing, character composition, and normal clipboard pasting will never trigger auto-send.  
After voice composition concludes, the system counts down the configured delay (default 0.5s, resetting if the IME continues AI formatting), ensuring AI-assisted voice keyboards have ample time to output final polished text.  
Because Android anonymizes recording sources for standard apps, an unrelated app starting a recording at the exact moment of focus represents a very minor theoretical false-positive boundary. Full details are viewable anytime via the info icon next to "Voice Auto-send Delay" in Settings.

---

## Technical Implementation and Constraints

| Module | Current Implementation | Constraints & Safety Boundaries |
| :--- | :--- | :--- |
| **Architecture** | Android phone client built with Flutter; desktop daemon built in pure Rust for Windows and macOS | Strictly no Electron, Python runtime, or Flutter desktop shells, ensuring minimal resource footprint |
| **Android Native Bridge** | Lightweight native Kotlin module: handles non-VPN physical Wi-Fi binding, OkHttp WebSocket connections, anonymous recording state monitoring, and IME subtype inspection | UI rendering, reactive state, and gestures remain in Flutter; never records audio, requests no microphone permissions |
| **Transport & Protocol** | Phone connects to desktop TCP `9618` via plain LAN WebSockets; messages use a fixed JSON schema | No cloud relays, no user accounts, no external internet dependency; use only on trusted LANs or hotspots; do not expose port to the public internet |
| **Desktop Listener** | Daemon listens on `0.0.0.0:9618`, never binding exclusively to a single network interface | Ethernet, Wi-Fi, and hotspots work seamlessly as long as the endpoint is unicast-reachable |
| **Pairing Mechanism** | Desktop displays a system DPI-scaled (~260pt) crisp QR code alongside a copyable `IP:port`; adapter IP switchable directly below QR | Scanning is not the sole gateway; phone supports manual IP entry and persists candidate IP pools per computer |
| **Multi-Device Management** | Each computer maintains an independent WebSocket connection with its own reconnect logic; phone broadcasts to all checked active targets | Input box on phone clears immediately once any checked target succeeds; does not block on ACK from every machine |
| **Text Injection Pipeline** | Text finishes composition on the phone keyboard; desktop writes plain text to clipboard, then injects genuine `Ctrl+V` / `Cmd+V`. If Automatic Enter is active and paste succeeds, system waits 80ms for the target app to digest text before sending Enter | Desktop receives standard paste shortcuts rather than simulated keystrokes for `v`; secure fields or apps disabling paste may reject input. Normal text sync and manual Enter bypass this 80ms delay |
| **Voice Detection Logic** | Focus must be accompanied by active Android recording or an IME subtype declaring `voice`; cursor must be at end of text, with no selection and finished composition | Relies on low-level source evidence rather than text heuristics, preventing typing/paste false positives and supporting standard voice IMEs without whitelists. Configurable stability delay (0 / 0.5s / 1s / 1.5s, default 0.5s) allows AI restructuring time. Recording detection requires Android 7+; Android 5/6 relies on `voice` subtype. Concurrent unrelated recording is the remaining theoretical edge case |
| **Shortcut Injection** | Phone sends key + modifiers; Windows calls `SendInput`, macOS calls `CGEvent` to inject genuine keydown / keyup events | One unified global shortcut mapping; no per-application configuration profiles |
| **Undo Mechanism** | Phone instructs target computers to inject `Ctrl+Z` on Windows or `Cmd+Z` on macOS, restoring the last sent draft back into the phone input | Undo efficacy depends on target application undo history; terminals and CLI tools may ignore or reinterpret shortcut; executed commands and terminal output cannot be rolled back |
| **Pointer & Cursor Control** | Phone transmits relative deltas, button bitmaps, and wheel ticks; default rates match display's **peak supported refresh rate** (60Hz or 120Hz; ≥90Hz defaults to 120Hz, failure defaults to 60Hz), with manual 240Hz option; Android window requests peak refresh to prevent frame throttling; sliders adjust pointer speed (×1–×4, default ×2) and wheel speed (×4–×28, default ×16); injected via `SendInput` / `CGEvent` | Strictly no fabricated path interpolation. When multiple deltas arrive within a single display refresh, cursor may appear to jump — this is delayed motion drawn at once, not misplacement; round trips maintain strict zero-drift precision. Trackpad / trackball / pointing stick share speed & wheel side configs; not physical HID hardware, not intended for esports or multi-display absolute mapping |
| **Local State Persistence** | Android stores device lists, selection states, shortcuts, themes, voice delays, input heights, pointer modes/sizes/speeds/rates, and wheel speed/side via `SharedPreferences` | Zero remote account synchronization; 100% of configuration and historical data remains strictly on the local device |
| **System Permissions** | Android requires network access and camera permission for QR scanning; voice detection requires no microphone permission; macOS requires "Accessibility" under System Settings for key and cursor injection | macOS Accessibility TCC and code signing are separate mechanisms; Windows Firewall must allow LAN TCP 9618 traffic |

---

## Log Locations

If you encounter connection issues or unexpected behavior, access logs via the desktop tray menu by clicking "Open Logs":

- **Windows**: `%APPDATA%\AgentPad\logs\`
- **macOS**: `~/Library/Logs/AgentPad/`

---

## Automatic Update Mechanism

AgentPad incorporates a lightweight and dependable cross-platform auto-update channel powered directly by GitHub Releases public APIs, free from third-party server dependencies:

- **Visual Update Indicator**:
  - The desktop pairing window permanently displays the running version number in the header alongside the listening port;
  - When a newer release is detected, a **green status dot** and an interactive "**Update**" button appear. Hovering reveals the release notes.
- **Windows (Dedicated Script & Safe Handover)**:
  - Clicking "Update" generates a dedicated updater script (`agentpad_updater.bat`) in the application directory and spawns it in the background;
  - The main process exits gracefully to release the executable file lock. The script waits for process termination, uses native `curl` to download the latest `agentpad-windows-x64.exe`, performs an in-place binary overwrite, restarts the new version, and self-destructs;
  - The newly launched version performs a **startup security check**: it verifies the embedded signature of any leftover updater script before deleting it, preventing accidental removal of user files.
- **macOS (Unix In-Place Atomic Replacement)**:
  - When an update is ready, clicking "Update" downloads the pre-built application archive (`agentpad-macos-arm64.zip`) in the background;
  - Leveraging macOS Unix inode replacement semantics, the extracted app atomically overwrites `/Applications/AgentPad.app` via system `ditto`;
  - Spawns the new version via `open -n` and terminates the old process. The upgrade completes within 2 seconds without requiring manual DMG mounting.
- **Android (In-App One-Tap Upgrade)**:
  - The bottom of the Settings panel displays the current version and a "Check for Updates" button;
  - Finding a new release prompts a dialog with release notes. Tapping "Download & Update" fetches the latest `agentpad.apk` and triggers the system package installer for a seamless upgrade, retaining all saved device pools and preferences.

---


## Project Structure

```text
AgentPad/
├── desktop/                 # Desktop daemon (Pure Rust core: Windows / macOS)
│   └── crates/
│       ├── agentpad/        # System tray, pairing UI window, WebSocket server
│       └── agentpad-input/  # Plain text injection, system keystrokes & cursor engine
├── android/                 # Mobile client (Android application)
│   ├── lib/                 # Flutter UI, state management, gesture recognizers, protocol client
│   └── android/app/src/main/kotlin/
│       └── app/agentpad/    # Physical Wi-Fi binding, WebSocket client, voice evidence native bridge
├── LICENSE
├── README.md
└── README.zh-CN.md
```

---

## License

This project is licensed under the [GNU Affero General Public License v3.0 (AGPL-3.0)](LICENSE).
