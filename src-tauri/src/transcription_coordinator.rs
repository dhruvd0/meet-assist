use crate::actions::ACTION_MAP;
use crate::managers::audio::AudioRecordingManager;
use crate::settings::ShortcutActivation;
use log::{debug, error, warn};
use std::sync::mpsc::{self, Sender};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

const DEBOUNCE: Duration = Duration::from_millis(30);
const RELEASE_GRACE: Duration = Duration::from_millis(50);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PttAction {
    Passthrough,
    DeferRelease,
    CancelRelease,
}

/// A key-up deferred by `RELEASE_GRACE` so a synthesized X11 auto-repeat
/// press can cancel it (#1539). When the grace elapses the hold is resolved
/// by [`CoordinatorState::finish_hold`] (recording) or
/// [`CoordinatorState::finish_pending_hold`] (press remembered while busy).
struct PendingRelease {
    binding_id: String,
    hotkey_string: String,
    deadline: Instant,
    /// When the key actually went up. The hold duration is measured to this
    /// instant, not to the grace expiry.
    released_at: Instant,
    /// Holds at least this long stop recording; shorter ones lock it on.
    /// Push-to-talk passes zero so every release stops.
    hold_threshold: Duration,
}

/// A press that arrived while the pipeline was still busy processing the
/// previous transcription. Toggle-style triggers (SIGUSR2, CLI flags, some
/// pedal setups) flip state on every edge, so dropping a busy press desyncs
/// the parity: the next edge starts a recording nobody will ever stop.
struct PendingPress {
    binding_id: String,
    hotkey_string: String,
    /// The real key-down time, so a hold that straddles the drain is still
    /// measured from when the user pressed, not from when recording began.
    pressed_at: Instant,
    /// The recording will start locked on when the pipeline drains: set from
    /// the start for toggle, and for hold-or-toggle once the key came back up
    /// within the threshold (a tap). An unlocked pending press is a key we
    /// believe is still held.
    locked: bool,
}

impl PendingPress {
    fn remembered(&self) -> Remembered {
        if self.locked {
            Remembered::Locked
        } else {
            Remembered::Held
        }
    }
}

/// What kind of press is already waiting for the pipeline to drain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Remembered {
    /// The key is still down as far as we know.
    Held,
    /// A toggle press or a classified tap: it will start a locked session.
    Locked,
}

/// Bookkeeping for the key press that started the current recording.
struct Hold {
    pressed_at: Instant,
    /// Recording outlives the key: the next press stops it, releases are
    /// ignored. Always set for toggle; set for hold-or-toggle once a release
    /// has been classified as a tap.
    locked: bool,
}

/// What to do with an input that arrives while the pipeline is busy
/// (`Stage::Processing`). `remembered` is the press for the same binding
/// already waiting for the pipeline to drain, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BusyAction {
    /// Ignore the input entirely.
    Ignore,
    /// Remember the press; start recording when the pipeline finishes.
    Remember,
    /// This press cancels a previously remembered press: two presses during
    /// one busy window net to no-op, exactly as a press stops a locked
    /// session once recording.
    Forget,
}

fn classify_busy_input(
    is_pressed: bool,
    mode: ShortcutActivation,
    remembered: Option<Remembered>,
) -> BusyAction {
    use ShortcutActivation::*;
    match (mode, is_pressed, remembered) {
        // Toggle: presses alternate remember/forget to preserve parity.
        (Toggle, true, Some(_)) => BusyAction::Forget,
        (Toggle, true, None) => BusyAction::Remember,
        // Toggle mode ignores releases.
        (Toggle, false, _) => BusyAction::Ignore,
        // Hold modes: a press while busy means the user is holding the key —
        // start as soon as the pipeline drains. A press on a queued tap stops
        // it (parity); a press while the key is already down is a repeat.
        (PushToTalk | HoldOrToggle, true, None) => BusyAction::Remember,
        (PushToTalk | HoldOrToggle, true, Some(Remembered::Locked)) => BusyAction::Forget,
        (PushToTalk | HoldOrToggle, true, Some(Remembered::Held)) => BusyAction::Ignore,
        // Releases of a held pending press are deferred by the grace window
        // before reaching here and resolved by `finish_pending_hold`; any
        // other release (no press remembered, or already locked) is noise.
        (PushToTalk | HoldOrToggle, false, _) => BusyAction::Ignore,
    }
}

/// Pipeline lifecycle.
#[derive(Debug, PartialEq, Eq)]
enum Stage {
    Idle,
    Recording(String), // binding_id
    Processing,
}

/// A keyboard/signal edge for a transcribe binding.
struct InputEvent {
    binding_id: String,
    hotkey_string: String,
    is_pressed: bool,
    mode: ShortcutActivation,
    /// Hold-or-toggle: minimum press duration that counts as a hold.
    hold_threshold: Duration,
    /// External triggers (SIGUSR2, CLI flags) rather than physical keys.
    /// They fire on every edge by design and must never be debounced —
    /// dropping one desyncs toggle parity and wedges recording on.
    external: bool,
}

impl InputEvent {
    /// The hold duration at or above which a release stops recording.
    fn effective_hold_threshold(&self) -> Duration {
        match self.mode {
            ShortcutActivation::HoldOrToggle => self.hold_threshold,
            // Every release stops; toggle never defers releases at all.
            ShortcutActivation::PushToTalk | ShortcutActivation::Toggle => Duration::ZERO,
        }
    }
}

/// A side effect decided by [`CoordinatorState`]; the coordinator thread is
/// the only executor. Keeping decisions pure lets tests drive the exact
/// production transitions without a Tauri `AppHandle` or real timers.
#[derive(Debug, PartialEq, Eq)]
enum Effect {
    Start {
        binding_id: String,
        hotkey_string: String,
    },
    Stop {
        binding_id: String,
        hotkey_string: String,
    },
}

/// Commands processed sequentially by the coordinator thread.
enum Command {
    Input(InputEvent),
    Cancel { recording_was_active: bool },
    ProcessingFinished,
}

/// Decide whether a key-up should be deferred (so auto-repeat can cancel it)
/// or a key-down cancels a deferred release. `hold_to_talk` is whether a
/// release currently ends the session: true for push-to-talk and for an
/// unlocked hold-or-toggle session, false for toggle and a locked session.
/// `held_binding` is the binding whose key we believe is down — the one
/// recording, or the one remembered while the pipeline is busy.
fn classify_ptt_event(
    pending_release_binding: Option<&str>,
    is_pressed: bool,
    hold_to_talk: bool,
    binding_id: &str,
    held_binding: Option<&str>,
) -> PttAction {
    if !hold_to_talk {
        return PttAction::Passthrough;
    }

    if is_pressed {
        if pending_release_binding == Some(binding_id) {
            PttAction::CancelRelease
        } else {
            PttAction::Passthrough
        }
    } else if held_binding == Some(binding_id) && pending_release_binding.is_none() {
        PttAction::DeferRelease
    } else {
        PttAction::Passthrough
    }
}

/// Pure lifecycle state machine: owns every transition decision (release
/// grace, hold-vs-tap classification, debounce, busy-pipeline
/// remember/forget, cancel, drain). Produces [`Effect`]s instead of touching
/// the app, so unit tests exercise the real production logic.
///
/// All three activation modes run through one machine. A recording starts on
/// key-down in every mode; what differs is how it ends:
///
/// * push-to-talk — every release stops (hold threshold of zero)
/// * toggle — releases are ignored, the next press stops (locked from the start)
/// * hold-or-toggle — a release after a long hold stops; a release after a
///   short tap locks the session, and the next press stops
struct CoordinatorState {
    stage: Stage,
    hold: Option<Hold>,
    last_press: Option<Instant>,
    pending_release: Option<PendingRelease>,
    pending_press: Option<PendingPress>,
}

impl CoordinatorState {
    fn new() -> Self {
        Self {
            stage: Stage::Idle,
            hold: None,
            last_press: None,
            pending_release: None,
            pending_press: None,
        }
    }

    /// Deadline of the deferred release, if any — drives `recv_timeout`.
    fn grace_deadline(&self) -> Option<Instant> {
        self.pending_release.as_ref().map(|p| p.deadline)
    }

    /// Whether the current session (recording, or remembered for the drain)
    /// outlives the key, so releases are ignored and the next press ends it.
    fn is_locked(&self) -> bool {
        self.hold.as_ref().is_some_and(|h| h.locked)
            || self.pending_press.as_ref().is_some_and(|p| p.locked)
    }

    fn on_input(&mut self, input: InputEvent, now: Instant) -> Option<Effect> {
        let pending_release_binding = self
            .pending_release
            .as_ref()
            .map(|pending| pending.binding_id.as_str());
        let held_binding = match &self.stage {
            Stage::Recording(id) => Some(id.as_str()),
            Stage::Processing => self.pending_press.as_ref().map(|p| p.binding_id.as_str()),
            Stage::Idle => None,
        };
        let hold_to_talk = input.mode != ShortcutActivation::Toggle && !self.is_locked();

        match classify_ptt_event(
            pending_release_binding,
            input.is_pressed,
            hold_to_talk,
            &input.binding_id,
            held_binding,
        ) {
            PttAction::CancelRelease => {
                self.pending_release = None;
                return None;
            }
            PttAction::DeferRelease => {
                self.pending_release = Some(PendingRelease {
                    hold_threshold: input.effective_hold_threshold(),
                    binding_id: input.binding_id,
                    hotkey_string: input.hotkey_string,
                    deadline: now + RELEASE_GRACE,
                    released_at: now,
                });
                return None;
            }
            PttAction::Passthrough => {}
        }

        // Debounce rapid-fire press events (key repeat / double-tap).
        // Releases in the hold modes are deferred above to absorb X11 auto-repeat.
        // External triggers are exempt: each one is a deliberate edge from the
        // user's own integration, and dropping it desyncs toggle parity.
        if input.is_pressed && !input.external {
            if self
                .last_press
                .is_some_and(|t| now.duration_since(t) < DEBOUNCE)
            {
                debug!("Debounced press for '{}'", input.binding_id);
                return None;
            }
            self.last_press = Some(now);
        }

        // A busy pipeline can't accept lifecycle changes now: classify the
        // input against any already-remembered press instead of dropping it
        // silently.
        if let Stage::Processing = self.stage {
            // Only one press can be remembered. Once a binding has claimed it,
            // inputs for a different binding are ignored — the same rule as a
            // different binding pressed while recording — rather than silently
            // replacing the remembered press and breaking its parity.
            if let Some(pending) = &self.pending_press {
                if pending.binding_id != input.binding_id {
                    debug!(
                        "Ignoring input for '{}': '{}' is already pending",
                        input.binding_id, pending.binding_id
                    );
                    return None;
                }
            }
            let remembered = self.pending_press.as_ref().map(|p| p.remembered());
            match classify_busy_input(input.is_pressed, input.mode, remembered) {
                BusyAction::Remember => {
                    debug!(
                        "Remembering press for '{}': pipeline busy",
                        input.binding_id
                    );
                    self.pending_press = Some(PendingPress {
                        // Toggle never ends on a release: locked from the start.
                        locked: input.mode == ShortcutActivation::Toggle,
                        binding_id: input.binding_id,
                        hotkey_string: input.hotkey_string,
                        pressed_at: now,
                    });
                }
                BusyAction::Forget => {
                    debug!("Forgetting remembered press for '{}'", input.binding_id);
                    self.pending_press = None;
                }
                BusyAction::Ignore => {
                    debug!("Ignoring input for '{}': pipeline busy", input.binding_id);
                }
            }
            return None;
        }

        if input.is_pressed {
            match &self.stage {
                Stage::Idle => {
                    // Toggle never ends on a release: locked from the start.
                    let locked = input.mode == ShortcutActivation::Toggle;
                    return Some(self.begin_recording(
                        input.binding_id,
                        input.hotkey_string,
                        now,
                        locked,
                    ));
                }
                Stage::Recording(id) if id == &input.binding_id => {
                    // A locked session ends on the next press. In toggle mode
                    // every press ends it, even if the recording began under a
                    // hold mode (the setting changed mid-recording) — otherwise
                    // nothing but Escape could stop it.
                    if self.is_locked() || input.mode == ShortcutActivation::Toggle {
                        return Some(self.begin_processing(input.binding_id, input.hotkey_string));
                    }
                    // The key is still held (its release will end this
                    // recording), so a repeated press means nothing.
                    debug!("Ignoring press for '{}': key is held", input.binding_id);
                }
                _ => debug!(
                    "Ignoring press for '{}': another binding is recording",
                    input.binding_id
                ),
            }
        } else if hold_to_talk
            && matches!(&self.stage, Stage::Recording(id) if id == &input.binding_id)
        {
            // A release that was not deferred (one is already pending for this
            // binding): resolve it immediately rather than dropping it.
            let threshold = input.effective_hold_threshold();
            return self.finish_hold(input.binding_id, input.hotkey_string, now, threshold);
        }
        None
    }

    /// The `RELEASE_GRACE` window elapsed with no cancelling press arriving:
    /// resolve the deferred release against whatever that binding's key was
    /// holding — the live recording, or a press remembered while busy.
    fn on_grace_expired(&mut self) -> Option<Effect> {
        let pending = self.pending_release.take()?;
        match &self.stage {
            Stage::Recording(id) if *id == pending.binding_id => self.finish_hold(
                pending.binding_id,
                pending.hotkey_string,
                pending.released_at,
                pending.hold_threshold,
            ),
            Stage::Processing => {
                self.finish_pending_hold(&pending);
                None
            }
            _ => None,
        }
    }

    /// A press remembered while the pipeline was busy has been released for
    /// real, still before the drain. A completed hold has nothing left to
    /// start; a tap queues a locked session so the drain starts it — the
    /// same hold-vs-tap rule as [`CoordinatorState::finish_hold`].
    fn finish_pending_hold(&mut self, release: &PendingRelease) {
        let Some(pending) = self
            .pending_press
            .as_mut()
            .filter(|p| p.binding_id == release.binding_id)
        else {
            return;
        };
        let held = release
            .released_at
            .saturating_duration_since(pending.pressed_at);
        if held < release.hold_threshold {
            debug!(
                "Tap ({held:?}) for '{}' while busy: will start locked on when the pipeline drains",
                release.binding_id
            );
            pending.locked = true;
        } else {
            debug!(
                "Forgetting remembered press for '{}': released after a {held:?} hold while busy",
                release.binding_id
            );
            self.pending_press = None;
        }
    }

    /// The key that started the current recording has been released for real.
    /// A hold at least `threshold` long stops recording; anything shorter was a
    /// tap, which locks the session on until the next press.
    fn finish_hold(
        &mut self,
        binding_id: String,
        hotkey_string: String,
        released_at: Instant,
        threshold: Duration,
    ) -> Option<Effect> {
        let held = self
            .hold
            .as_ref()
            .map(|h| released_at.saturating_duration_since(h.pressed_at))
            // No hold bookkeeping means we cannot tell a tap from a hold;
            // stopping is the safe reading (it is what push-to-talk always did).
            .unwrap_or(Duration::MAX);
        if held >= threshold {
            return Some(self.begin_processing(binding_id, hotkey_string));
        }
        if let Some(hold) = &mut self.hold {
            debug!("Tap ({held:?}) for '{binding_id}': recording locked on until the next press");
            hold.locked = true;
        }
        None
    }

    fn on_cancel(&mut self, recording_was_active: bool) {
        self.pending_release = None;
        // An explicit cancel abandons any remembered start too — the user
        // asked for silence, not a deferred recording.
        self.pending_press = None;
        // Don't reset during processing — wait for the pipeline to finish.
        if !matches!(self.stage, Stage::Processing)
            && (recording_was_active || matches!(self.stage, Stage::Recording(_)))
        {
            self.stage = Stage::Idle;
            self.hold = None;
        }
    }

    fn on_processing_finished(&mut self) -> Option<Effect> {
        self.stage = Stage::Idle;
        self.hold = None;
        let pending = self.pending_press.take()?;
        debug!(
            "Pipeline drained; starting remembered press for '{}'",
            pending.binding_id
        );
        Some(self.begin_recording(
            pending.binding_id,
            pending.hotkey_string,
            pending.pressed_at,
            pending.locked,
        ))
    }

    /// Reconcile the optimistic `Stage::Recording` after the executor reports
    /// whether recording actually began (microphone access can be denied).
    fn on_start_result(&mut self, binding_id: &str, started: bool) {
        if !started && matches!(&self.stage, Stage::Recording(id) if id == binding_id) {
            self.stage = Stage::Idle;
            self.hold = None;
        }
    }

    /// Optimistic transition to `Recording`; rolled back via
    /// [`CoordinatorState::on_start_result`] if the effect fails to start
    /// recording for real.
    fn begin_recording(
        &mut self,
        binding_id: String,
        hotkey_string: String,
        pressed_at: Instant,
        locked: bool,
    ) -> Effect {
        self.stage = Stage::Recording(binding_id.clone());
        self.hold = Some(Hold { pressed_at, locked });
        Effect::Start {
            binding_id,
            hotkey_string,
        }
    }

    fn begin_processing(&mut self, binding_id: String, hotkey_string: String) -> Effect {
        self.stage = Stage::Processing;
        self.hold = None;
        Effect::Stop {
            binding_id,
            hotkey_string,
        }
    }
}

/// Serialises all transcription lifecycle events through a single thread
/// to eliminate race conditions between keyboard shortcuts, signals, and
/// the async transcribe-paste pipeline. The thread is a thin shell: it
/// transports commands to the pure [`CoordinatorState`] and executes the
/// returned [`Effect`]s.
pub struct TranscriptionCoordinator {
    tx: Sender<Command>,
}

pub fn is_transcribe_binding(id: &str) -> bool {
    id == "transcribe" || id == "transcribe_with_post_process"
}

impl TranscriptionCoordinator {
    pub fn new(app: AppHandle) -> Self {
        let (tx, rx) = mpsc::channel();

        thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut state = CoordinatorState::new();

                loop {
                    let cmd = if let Some(deadline) = state.grace_deadline() {
                        match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                            Ok(cmd) => cmd,
                            Err(mpsc::RecvTimeoutError::Timeout) => {
                                if let Some(effect) = state.on_grace_expired() {
                                    run_effect(&app, &mut state, effect);
                                }
                                continue;
                            }
                            Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        }
                    } else {
                        match rx.recv() {
                            Ok(cmd) => cmd,
                            Err(_) => break,
                        }
                    };

                    match cmd {
                        Command::Input(input) => {
                            if let Some(effect) = state.on_input(input, Instant::now()) {
                                run_effect(&app, &mut state, effect);
                            }
                        }
                        Command::Cancel {
                            recording_was_active,
                        } => state.on_cancel(recording_was_active),
                        Command::ProcessingFinished => {
                            if let Some(effect) = state.on_processing_finished() {
                                run_effect(&app, &mut state, effect);
                            }
                        }
                    }
                }
                debug!("Transcription coordinator exited");
            }));
            if let Err(e) = result {
                error!("Transcription coordinator panicked: {e:?}");
            }
        });

        Self { tx }
    }

    /// Send a keyboard input event for a transcribe binding. `hold_threshold`
    /// only matters for [`ShortcutActivation::HoldOrToggle`].
    pub fn send_input(
        &self,
        binding_id: &str,
        hotkey_string: &str,
        is_pressed: bool,
        mode: ShortcutActivation,
        hold_threshold: Duration,
    ) {
        self.send(
            binding_id,
            hotkey_string,
            is_pressed,
            mode,
            hold_threshold,
            false,
        );
    }

    /// Send an external trigger (SIGUSR2, CLI flag). Always a toggle press,
    /// always exempt from debounce — see [`InputEvent::external`].
    pub fn send_external_input(&self, binding_id: &str, source: &str) {
        self.send(
            binding_id,
            source,
            true,
            ShortcutActivation::Toggle,
            Duration::ZERO,
            true,
        );
    }

    fn send(
        &self,
        binding_id: &str,
        hotkey_string: &str,
        is_pressed: bool,
        mode: ShortcutActivation,
        hold_threshold: Duration,
        external: bool,
    ) {
        if self
            .tx
            .send(Command::Input(InputEvent {
                binding_id: binding_id.to_string(),
                hotkey_string: hotkey_string.to_string(),
                is_pressed,
                mode,
                hold_threshold,
                external,
            }))
            .is_err()
        {
            warn!("Transcription coordinator channel closed");
        }
    }

    pub fn notify_cancel(&self, recording_was_active: bool) {
        if self
            .tx
            .send(Command::Cancel {
                recording_was_active,
            })
            .is_err()
        {
            warn!("Transcription coordinator channel closed");
        }
    }

    pub fn notify_processing_finished(&self) {
        if self.tx.send(Command::ProcessingFinished).is_err() {
            warn!("Transcription coordinator channel closed");
        }
    }
}

fn run_effect(app: &AppHandle, state: &mut CoordinatorState, effect: Effect) {
    match effect {
        Effect::Start {
            binding_id,
            hotkey_string,
        } => {
            let started = start(app, &binding_id, &hotkey_string);
            state.on_start_result(&binding_id, started);
        }
        Effect::Stop {
            binding_id,
            hotkey_string,
        } => stop(app, &binding_id, &hotkey_string),
    }
}

/// Execute a start effect; returns whether recording actually began, so the
/// state machine can roll back its optimistic transition on failure.
fn start(app: &AppHandle, binding_id: &str, hotkey_string: &str) -> bool {
    let Some(action) = ACTION_MAP.get(binding_id) else {
        warn!("No action in ACTION_MAP for '{binding_id}'");
        return false;
    };
    action.start(app, binding_id, hotkey_string);
    let recording = app
        .try_state::<Arc<AudioRecordingManager>>()
        .is_some_and(|a| a.is_recording());
    if !recording {
        debug!("Start for '{binding_id}' did not begin recording; staying idle");
    }
    recording
}

fn stop(app: &AppHandle, binding_id: &str, hotkey_string: &str) {
    let Some(action) = ACTION_MAP.get(binding_id) else {
        warn!("No action in ACTION_MAP for '{binding_id}'");
        return;
    };
    action.stop(app, binding_id, hotkey_string);
}
