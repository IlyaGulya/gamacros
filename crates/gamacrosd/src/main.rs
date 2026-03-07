mod app;
mod activity;
mod api;
mod cli;
mod domain;
mod logging;
mod runner;
#[cfg(target_os = "macos")]
mod accessibility;

use std::fs::File;
use std::path::PathBuf;
use std::{process, time::Duration};

use clap::Parser;
use colored::Colorize;
use crossbeam_channel::{select, unbounded};
use lunchctl::{LaunchAgent, LaunchControllable};
use crate::activity::{ActivityEvent, Monitor, NotificationListener};

use gamacros_control::Performer;
use gamacros_gamepad::{ControllerEvent, ControllerManager};
use gamacros_workspace::{ProfileEvent, Workspace};

use crate::api::{ApiTransport, Command as ApiCommand, UnixSocket};
use crate::app::{ButtonPhase, Gamacros};
use crate::cli::{Cli, Command, ControlCommand};
use crate::domain::{DomainEvent, SystemEvent, TimerEvent};
use crate::runner::ActionRunner;

const APP_LABEL: &str = "co.myrt.gamacros";

fn main() -> process::ExitCode {
    let cli = Cli::parse();
    if cli.command != Command::Observe {
        logging::setup(cli.verbose, cli.no_color);
    }

    let bin_path = std::env::current_exe().unwrap();

    match cli.command {
        Command::Run { workspace } => {
            let workspace_path = resolve_workspace_path(workspace.as_deref());
            #[cfg(target_os = "macos")]
            {
                let _ = accessibility::request_if_needed();
            }
            run_event_loop(Some(workspace_path));
        }
        Command::Start { workspace } => {
            let workspace_path = resolve_workspace_path(workspace.as_deref());

            #[cfg(target_os = "macos")]
            {
                let _ = accessibility::request_if_needed();
            }

            let mut arguments = vec![bin_path.display().to_string()];
            if cli.verbose {
                arguments.push("--verbose".to_string());
            }
            arguments.push("run".to_string());
            arguments.push("--workspace".to_string());
            arguments.push(workspace_path.display().to_string());

            let agent = LaunchAgent {
                label: APP_LABEL.to_string(),
                program_arguments: arguments,
                standard_out_path: "/tmp/gamacros.out".to_string(),
                standard_error_path: "/tmp/gamacros.err".to_string(),
                keep_alive: true,
                run_at_load: true,
            };

            if let Err(e) = agent.write() {
                print_error!("Failed to write agent: {}", e);
                return process::ExitCode::FAILURE;
            }

            match agent.is_running() {
                Ok(true) => {
                    print_info!("Agent is already running");
                }
                Ok(false) => {
                    print_info!("Starting agent");
                    if let Err(e) = agent.bootstrap() {
                        print_error!("Failed to bootstrap agent: {}", e);
                        return process::ExitCode::FAILURE;
                    }
                    print_info!("Agent started");
                }
                Err(e) => {
                    print_error!("Failed to check if agent is running: {}", e);
                    return process::ExitCode::FAILURE;
                }
            }
        }
        Command::Stop => {
            if !LaunchAgent::exists(APP_LABEL) {
                print_error!("Agent does not exist");
                return process::ExitCode::FAILURE;
            }

            let agent = LaunchAgent::from_file(APP_LABEL).unwrap();

            match agent.is_running() {
                Ok(true) => {
                    print_info!("Stopping agent");
                    if let Err(e) = agent.boot_out() {
                        print_error!("Failed to stop agent: {}", e);
                        return process::ExitCode::FAILURE;
                    }
                    print_info!("Agent stopped");
                }
                Ok(false) => {
                    print_info!("Agent is not running");
                }
                Err(e) => {
                    print_error!("Failed to check if agent is running: {}", e);
                    return process::ExitCode::FAILURE;
                }
            }
        }
        Command::Status => {
            if !LaunchAgent::exists(APP_LABEL) {
                print_info!("Agent does not exist");
                return process::ExitCode::FAILURE;
            }

            let agent = LaunchAgent::from_file(APP_LABEL).unwrap();
            match agent.is_running() {
                Ok(true) => {
                    print_info!("Agent is running");
                }
                Ok(false) => {
                    print_info!("Agent is not running");
                }
                Err(e) => {
                    print_error!("Failed to check if agent is running: {}", e);
                    return process::ExitCode::FAILURE;
                }
            }
        }
        Command::Observe => {
            logging::setup(true, cli.no_color);
            #[cfg(target_os = "macos")]
            {
                let _ = accessibility::request_if_needed();
            }
            run_event_loop(None);
        }
        Command::Command { workspace, command } => match command {
            ControlCommand::Rumble { id, ms } => {
                let workspace_path = resolve_workspace_path(workspace.as_deref());
                match UnixSocket::new(workspace_path)
                    .send_event(ApiCommand::Rumble { id, ms })
                {
                    Ok(_) => {
                        print_info!("Rumbled controller {:?} for {ms}ms", id);
                    }
                    Err(e) => {
                        print_error!("failed to send rumble command: {e}");
                    }
                };
            }
        },
    }

    process::ExitCode::SUCCESS
}

fn resolve_workspace_path(workspace: Option<&str>) -> PathBuf {
    let workspace = workspace.map(PathBuf::from);
    if let Some(workspace) = workspace {
        // If the user passed a file path (e.g. gc_profile.yaml), use its parent directory
        if workspace.is_file() {
            return workspace.parent().unwrap_or(&workspace).to_owned();
        }
        return workspace;
    }

    match Workspace::default_path() {
        Ok(path) => path,
        Err(e) => {
            print_error!("failed to resolve workspace: {e}");

            process::exit(1);
        }
    }
}

enum EventLoopControl {
    Continue,
    Break,
}

fn run_effects(
    action_runner: &mut ActionRunner<'_>,
    effects: Vec<crate::app::Effect>,
) {
    for effect in effects {
        action_runner.run_effect(effect);
    }
}

struct WakeState {
    need_reschedule: bool,
    ticking_enabled: bool,
    fast_mode: bool,
    fast_until: std::time::Instant,
    next_tick_due: Option<std::time::Instant>,
}

impl WakeState {
    fn new(now: std::time::Instant) -> Self {
        Self {
            need_reschedule: true,
            ticking_enabled: false,
            fast_mode: false,
            fast_until: now,
            next_tick_due: None,
        }
    }
}

fn handle_domain_event(
    event: DomainEvent,
    gamacros: &mut Gamacros,
    action_runner: &mut ActionRunner<'_>,
    manager: &ControllerManager,
    wake_state: &mut WakeState,
) -> EventLoopControl {
    match event {
        DomainEvent::Controller(controller_event) => match controller_event {
            ControllerEvent::Connected(info) => {
                let id = info.id;
                if gamacros.is_known(id) {
                    return EventLoopControl::Continue;
                }

                gamacros.add_controller(info);
                wake_state.need_reschedule = true;
            }
            ControllerEvent::Disconnected(id) => {
                gamacros.remove_controller(id);
                gamacros.on_controller_disconnected(id);
                wake_state.need_reschedule = true;
            }
            ControllerEvent::ButtonPressed { id, button } => {
                run_effects(
                    action_runner,
                    gamacros.on_button_effects(id, button, ButtonPhase::Pressed),
                );
                wake_state.need_reschedule = true;
            }
            ControllerEvent::ButtonReleased { id, button } => {
                run_effects(
                    action_runner,
                    gamacros.on_button_effects(id, button, ButtonPhase::Released),
                );
                wake_state.need_reschedule = true;
            }
            ControllerEvent::AxisMotion { id, axis, value } => {
                print_debug!(
                    "domain event: axis motion controller={id} axis={axis:?} value={value:.3}"
                );
                gamacros.on_axis_motion(id, axis, value);
                if !wake_state.ticking_enabled {
                    wake_state.need_reschedule = true;
                    print_debug!(
                        "axis motion armed ticking: controller={id} axis={axis:?} value={value:.3}"
                    );
                }
            }
        },
        DomainEvent::Activity(activity_event) => {
            let ActivityEvent::DidActivateApplication(bundle_id) = activity_event
            else {
                return EventLoopControl::Continue;
            };
            gamacros.set_active_app(&bundle_id);
            action_runner.set_shell(gamacros.current_shell());
            wake_state.need_reschedule = true;
        }
        DomainEvent::Profile(profile_event) => match profile_event {
            ProfileEvent::Changed(workspace) => {
                print_info!("profile changed, updating workspace");
                gamacros.set_workspace(workspace);
                action_runner.set_shell(gamacros.current_shell());
                wake_state.need_reschedule = true;
            }
            ProfileEvent::Removed => {
                gamacros.remove_workspace();
                action_runner.set_shell(gamacros.current_shell());
                wake_state.need_reschedule = true;
            }
            ProfileEvent::Error(error) => {
                print_error!("profile error: {error}");
            }
        },
        DomainEvent::Api(command) => match command {
            ApiCommand::Rumble { id, ms } => match id {
                Some(controller_id) => {
                    action_runner.run_effect(crate::app::Effect::Rumble {
                        id: controller_id,
                        ms,
                    });
                }
                None => {
                    for info in manager.controllers() {
                        action_runner.run_effect(crate::app::Effect::Rumble {
                            id: info.id,
                            ms,
                        });
                    }
                }
            },
        },
        DomainEvent::Timer(TimerEvent::Wake) => {
            let now = std::time::Instant::now();
            print_debug!(
                "wake timer fired: next_tick_due={:?} fast_mode={} need_reschedule={}",
                wake_state.next_tick_due,
                wake_state.fast_mode,
                wake_state.need_reschedule
            );
            if let Some(due) = wake_state.next_tick_due {
                if now >= due {
                    print_debug!(
                        "wake timer: tick due lateness_us={}",
                        now.duration_since(due).as_micros()
                    );
                    run_effects(action_runner, gamacros.on_tick_effects());
                    if gamacros.wants_fast_tick() {
                        wake_state.fast_mode = true;
                        wake_state.fast_until = now + Duration::from_millis(250);
                        print_debug!(
                            "wake timer: enabling fast mode until {:?}",
                            wake_state.fast_until
                        );
                    } else if wake_state.fast_mode && now >= wake_state.fast_until {
                        wake_state.fast_mode = false;
                        print_debug!("wake timer: disabling fast mode");
                    }
                }
            }
            let repeats_started_at = std::time::Instant::now();
            run_effects(action_runner, gamacros.due_repeat_effects(now));
            print_debug!(
                "wake timer: stick repeats elapsed_us={}",
                repeats_started_at.elapsed().as_micros()
            );
            let button_repeats_started_at = std::time::Instant::now();
            run_effects(action_runner, gamacros.button_repeat_effects(now));
            print_debug!(
                "wake timer: button repeats elapsed_us={}",
                button_repeats_started_at.elapsed().as_micros()
            );
            wake_state.need_reschedule = true;
        }
        DomainEvent::System(SystemEvent::ShutdownRequested) => {
            return EventLoopControl::Break;
        }
    }

    EventLoopControl::Continue
}

fn process_overdue_wake(
    gamacros: &mut Gamacros,
    action_runner: &mut ActionRunner<'_>,
    manager: &ControllerManager,
    wake_state: &mut WakeState,
) -> EventLoopControl {
    let now = std::time::Instant::now();
    let tick_due = wake_state.next_tick_due.is_some_and(|due| due <= now);
    let stick_repeat_due = gamacros.next_repeat_due().is_some_and(|due| due <= now);
    let button_repeat_due = gamacros
        .next_button_repeat_due()
        .is_some_and(|due| due <= now);

    if tick_due || stick_repeat_due || button_repeat_due {
        print_debug!(
            "processing overdue wake: tick_due={} stick_repeat_due={} button_repeat_due={}",
            tick_due,
            stick_repeat_due,
            button_repeat_due
        );
        return handle_domain_event(
            DomainEvent::Timer(TimerEvent::Wake),
            gamacros,
            action_runner,
            manager,
            wake_state,
        );
    }

    EventLoopControl::Continue
}

fn run_event_loop(maybe_workspace_path: Option<PathBuf>) {
    // Ensure only one instance runs per workspace by holding an exclusive flock.
    if let Some(ws) = maybe_workspace_path.as_ref() {
        let lock_path = ws.parent().unwrap_or(ws).join("gamacrosd.lock");
        let file = File::create(&lock_path).unwrap_or_else(|e| {
            print_error!("failed to create lock file {}: {e}", lock_path.display());
            process::exit(1);
        });
        let mut lock = fd_lock::RwLock::new(file);
        match lock.try_write() {
            Ok(guard) => {
                // Forget the guard so Drop doesn't unlock.
                // The fd stays open via `lock` (kept alive below), so the OS lock persists
                // until the process exits.
                std::mem::forget(guard);
            }
            Err(_) => {
                print_error!(
                    "another gamacrosd is already running (lock: {})",
                    lock_path.display()
                );
                process::exit(1);
            }
        }
        // Leak the RwLock to keep the fd open for the process lifetime.
        std::mem::forget(lock);
    }

    #[cfg(target_os = "macos")]
    {
        let trusted = accessibility::is_trusted();
        print_info!("accessibility trusted: {trusted}");
        if !trusted {
            accessibility::log_missing_permission();
            loop {
                std::thread::sleep(Duration::from_secs(1));
                let trusted = accessibility::is_trusted();
                if trusted {
                    print_info!("accessibility trusted: {trusted}");
                    break;
                }
            }
        }
    }

    // Activity monitor must run on the main thread.
    // We keep its std::mpsc receiver and poll it from the event loop (no bridge thread).
    let Some((monitor, activity_std_rx, monitor_stop_tx)) = Monitor::new() else {
        print_error!("failed to start activity monitor");
        return;
    };

    monitor.subscribe(NotificationListener::DidActivateApplication);
    let mut gamacros = Gamacros::new();
    if let Some(app) = monitor.get_active_application() {
        gamacros.set_active_app(&app)
    }

    // Handle Ctrl+C to exit cleanly
    let (stop_tx, stop_rx) = unbounded::<()>();
    ctrlc::set_handler(move || {
        let _ = stop_tx.send(());
        let _ = monitor_stop_tx.send(());
    })
    .expect("failed to set Ctrl+C handler");

    let workspace_path = maybe_workspace_path.to_owned();

    // Start control socket on the main thread and forward commands into the event loop.
    let (api_tx, api_rx) = unbounded::<ApiCommand>();
    let _control_handle = workspace_path.clone().and_then(|ws_path| {
        let sock_dir = ws_path.parent().unwrap_or(&ws_path).to_owned();
        match UnixSocket::new(sock_dir).listen_events(api_tx) {
            Ok(h) => Some(h),
            Err(e) => {
                print_error!("failed to start api server: {e}");
                None
            }
        }
    });

    // Run the main event loop in a background thread while the main thread runs the monitor loop.
    let event_loop = std::thread::Builder::new()
        .name("event-loop".into())
        .stack_size(512 * 1024)
        .spawn(move || {
            let manager = ControllerManager::new()
                .expect("failed to start controller manager");
            let rx = manager.subscribe();
            let mut keypress = Performer::new().expect("failed to start keypress");
            // Single coalesced wake timer: earliest of movement tick and repeat deadlines.
            let mut wake_rx = crossbeam_channel::never::<std::time::Instant>();
            let idle_period = Duration::from_millis(16);
            let fast_period = Duration::from_millis(5);
            let mut wake_state = WakeState::new(std::time::Instant::now());

            let workspace = match Workspace::new(workspace_path.as_deref()) {
                Ok(workspace) => workspace,
                Err(e) => {
                    print_error!("failed to start workspace: {e}");
                    return;
                }
            };

            let maybe_watcher = workspace_path
                .as_ref()
                .map(|_| workspace.start_profile_watcher())
                .transpose()
                .expect("failed to start workspace watcher");

            let (maybe_workspace_rx, _profile_watcher) = match maybe_watcher {
                Some((watcher, rx)) => (Some(rx), Some(watcher)),
                None => (None, None),
            };

            let mut action_runner = ActionRunner::new(&mut keypress, &manager);

            print_info!(
                "gamacrosd started. Listening for controller and activity events."
            );
            loop {
                select! {
                    recv(stop_rx) -> _ => {
                        if let EventLoopControl::Break = handle_domain_event(
                            DomainEvent::System(SystemEvent::ShutdownRequested),
                            &mut gamacros,
                            &mut action_runner,
                            &manager,
                            &mut wake_state,
                        ) {
                            break;
                        }
                    }
                    recv(rx) -> msg => {
                        match msg {
                            Ok(event) => {
                            if let EventLoopControl::Break = handle_domain_event(
                                DomainEvent::Controller(event),
                                &mut gamacros,
                                &mut action_runner,
                                &manager,
                                &mut wake_state,
                            ) {
                                break;
                            }
                            if let EventLoopControl::Break = process_overdue_wake(
                                &mut gamacros,
                                &mut action_runner,
                                &manager,
                                &mut wake_state,
                            ) {
                                break;
                            }
                        }
                        Err(err) => {
                            print_error!("event channel closed: {err}");
                                break;
                            }
                        }
                    }
                    recv(api_rx) -> cmd => {
                        if let Ok(command) = cmd {
                            if let EventLoopControl::Break = handle_domain_event(
                                DomainEvent::Api(command),
                                &mut gamacros,
                                &mut action_runner,
                                &manager,
                                &mut wake_state,
                            ) {
                                break;
                            }
                        }
                    }
                    recv(wake_rx) -> _ => {
                        if let EventLoopControl::Break = handle_domain_event(
                            DomainEvent::Timer(TimerEvent::Wake),
                            &mut gamacros,
                            &mut action_runner,
                            &manager,
                            &mut wake_state,
                        ) {
                            break;
                        }
                    }
                }
                while let Ok(msg) = activity_std_rx.try_recv() {
                    if let EventLoopControl::Break = handle_domain_event(
                        DomainEvent::Activity(msg),
                        &mut gamacros,
                        &mut action_runner,
                        &manager,
                        &mut wake_state,
                    ) {
                        break;
                    }
                    if let EventLoopControl::Break = process_overdue_wake(
                        &mut gamacros,
                        &mut action_runner,
                        &manager,
                        &mut wake_state,
                    ) {
                        break;
                    }
                }
                let Some(workspace_rx) = maybe_workspace_rx.as_ref() else {
                    continue;
                };

                while let Ok(msg) = workspace_rx.try_recv() {
                    if let EventLoopControl::Break = handle_domain_event(
                        DomainEvent::Profile(msg),
                        &mut gamacros,
                        &mut action_runner,
                        &manager,
                        &mut wake_state,
                    ) {
                        break;
                    }
                    if let EventLoopControl::Break = process_overdue_wake(
                        &mut gamacros,
                        &mut action_runner,
                        &manager,
                        &mut wake_state,
                    ) {
                        break;
                    }
                }
                if wake_state.need_reschedule {
                    let now = std::time::Instant::now();
                    // Recompute next tick due
                    if gamacros.needs_tick() {
                        let was_ticking_enabled = wake_state.ticking_enabled;
                        let previous_tick_due = wake_state.next_tick_due;
                        if !wake_state.ticking_enabled {
                            wake_state.fast_mode = gamacros.wants_fast_tick();
                            if wake_state.fast_mode {
                                wake_state.fast_until =
                                    now + Duration::from_millis(250);
                            }
                        }
                        let period = if wake_state.fast_mode {
                            if gamacros.wants_ultra_fast_tick() {
                                Duration::from_millis(
                                    gamacros.mouse_move_tick_ms().unwrap_or(4),
                                )
                            } else {
                                fast_period
                            }
                        } else {
                            idle_period
                        };
                        let desired_tick_due = now + period;
                        wake_state.next_tick_due = match previous_tick_due {
                            Some(existing_due)
                                if was_ticking_enabled && existing_due > now =>
                            {
                                Some(core::cmp::min(existing_due, desired_tick_due))
                            }
                            _ => Some(desired_tick_due),
                        };
                        wake_state.ticking_enabled = true;
                        let next_tick_in = wake_state
                            .next_tick_due
                            .map(|due| due.saturating_duration_since(now).as_millis())
                            .unwrap_or_default();
                        print_debug!(
                            "wake reschedule: ticking_enabled=true fast_mode={} next_tick_in_ms={}",
                            wake_state.fast_mode,
                            next_tick_in
                        );
                    } else {
                        wake_state.next_tick_due = None;
                        wake_state.ticking_enabled = false;
                        wake_state.fast_mode = false;
                        print_debug!("wake reschedule: ticking disabled");
                    }
                    // Recompute next repeat due (sticks + buttons)
                    let repeat_due = gamacros.next_repeat_due();
                    let button_repeat_due = gamacros.next_button_repeat_due();

                    // Arm single wake for the earliest deadline
                    let mut next_due = wake_state.next_tick_due;
                    for candidate in [repeat_due, button_repeat_due] {
                        next_due = match (next_due, candidate) {
                            (Some(a), Some(b)) => Some(core::cmp::min(a, b)),
                            (Some(a), None) => Some(a),
                            (None, b) => b,
                        };
                    }
                    if let Some(due) = next_due {
                        let dur = if due > now { due - now } else { Duration::ZERO };
                        print_debug!(
                            "wake arm: next_due={due:?} in_ms={} tick_due={:?} stick_repeat_due={repeat_due:?} button_repeat_due={button_repeat_due:?}",
                            dur.as_millis(),
                            wake_state.next_tick_due
                        );
                        wake_rx = crossbeam_channel::after(dur);
                    } else {
                        print_debug!("wake arm: no pending deadlines");
                        wake_rx = crossbeam_channel::never();
                    }
                    wake_state.need_reschedule = false;
                }
            }
        })
        .expect("failed to spawn event loop thread");

    // Start monitoring on the main thread (blocks until error/exit)
    monitor.run();
    if let Err(e) = event_loop.join() {
        print_error!("event loop error: {e:?}");
    }
}
