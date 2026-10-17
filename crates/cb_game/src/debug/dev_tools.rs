//! Interactive debug tools (feature `dev`, ADR-0031): perf overlay (F1), sim controls (F2),
//! log console (F3), inspector (F4). English only (developer-facing).

use std::collections::VecDeque;

use bevy::prelude::*;
use bevy::reflect::{PartialReflect, ReflectRef};
use bevy_egui::{EguiContexts, EguiPrimaryContextPass, egui};
use cb_sim::{
    CommandQueue, DayLength, GrowthMode, PlayerCommand, SimClock, SimConfig, SimDriver, SimPacing,
    SimSpeed, SimStepRequest,
};

use crate::bootstrap::logging::LogRing;

/// Shared log ring for the console.
#[derive(Resource, Clone)]
pub struct ConsoleRing(pub LogRing);

/// Which windows are open.
#[derive(Resource, Debug)]
pub struct DevUi {
    perf: bool,
    sim: bool,
    console: bool,
    inspector: bool,
}

impl Default for DevUi {
    fn default() -> Self {
        Self {
            perf: true,
            sim: true,
            console: false,
            inspector: false,
        }
    }
}

/// Recent real frame times (ms).
#[derive(Resource, Default, Debug)]
struct FrameTimes(VecDeque<f64>);

/// Reflected sim resources, refreshed while the inspector is open.
#[derive(Resource, Default, Debug)]
struct InspectorSnapshot(Vec<(String, String)>);

/// Dev tools plugin.
pub struct DevToolsPlugin;

impl Plugin for DevToolsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DevUi>()
            .init_resource::<FrameTimes>()
            .init_resource::<InspectorSnapshot>()
            .add_systems(PreUpdate, toggles)
            .add_systems(Last, (record_frame_time, snapshot_inspector))
            .add_systems(EguiPrimaryContextPass, windows);
    }
}

fn toggles(keys: Res<ButtonInput<KeyCode>>, mut ui: ResMut<DevUi>) {
    let ui = &mut *ui;
    for (key, flag) in [
        (KeyCode::F1, &mut ui.perf),
        (KeyCode::F2, &mut ui.sim),
        (KeyCode::F3, &mut ui.console),
        (KeyCode::F4, &mut ui.inspector),
    ] {
        if keys.just_pressed(key) {
            *flag = !*flag;
        }
    }
}

fn record_frame_time(time: Res<Time<Real>>, mut times: ResMut<FrameTimes>) {
    times.0.push_back(time.delta_secs_f64() * 1000.0);
    if times.0.len() > 240 {
        times.0.pop_front();
    }
}

fn percentile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let i = ((sorted.len() - 1) as f64 * q).round() as usize;
    sorted[i.min(sorted.len() - 1)]
}

/// Field-by-field view of a reflected value (structs, tuple structs, enums).
fn reflect_fields(value: &dyn PartialReflect) -> String {
    match value.reflect_ref() {
        ReflectRef::Struct(s) => (0..s.field_len())
            .filter_map(|i| Some(format!("{}: {:?}", s.name_at(i)?, s.field_at(i)?)))
            .collect::<Vec<_>>()
            .join("\n"),
        ReflectRef::TupleStruct(t) => (0..t.field_len())
            .filter_map(|i| Some(format!("{i}: {:?}", t.field(i)?)))
            .collect::<Vec<_>>()
            .join("\n"),
        ReflectRef::Enum(e) => format!("variant: {}", e.variant_name()),
        _ => format!("{value:?}"),
    }
}

fn snapshot_inspector(world: &mut World) {
    if !world.get_resource::<DevUi>().is_some_and(|u| u.inspector) {
        return;
    }
    fn row<R: Resource + PartialReflect>(world: &World, name: &str) -> Option<(String, String)> {
        world
            .get_resource::<R>()
            .map(|r| (name.to_owned(), reflect_fields(r)))
    }
    let rows: Vec<(String, String)> = [
        row::<SimClock>(world, "SimClock"),
        row::<SimConfig>(world, "SimConfig"),
        row::<cb_sim::WorldSeed>(world, "WorldSeed"),
        row::<SimSpeed>(world, "SimSpeed"),
        row::<SimPacing>(world, "SimPacing"),
    ]
    .into_iter()
    .flatten()
    .collect();
    world.resource_mut::<InspectorSnapshot>().0 = rows;
}

fn windows(
    mut contexts: EguiContexts,
    mut ui_state: ResMut<DevUi>,
    times: Res<FrameTimes>,
    driver: Res<SimDriver>,
    clock: Res<SimClock>,
    config: Res<SimConfig>,
    mut speed: ResMut<SimSpeed>,
    mut pacing: ResMut<SimPacing>,
    mut step: ResMut<SimStepRequest>,
    ring: Option<Res<ConsoleRing>>,
    snapshot: Res<InspectorSnapshot>,
    entities: Query<Entity>,
    mut queue: ResMut<CommandQueue>,
) {
    let Ok(ctx) = contexts.ctx_mut() else { return };
    let ui = &mut *ui_state;

    if ui.perf {
        let mut sorted: Vec<f64> = times.0.iter().copied().collect();
        sorted.sort_by(f64::total_cmp);
        let s = driver.stats;
        egui::Window::new("Performance").open(&mut ui.perf).default_pos([12.0, 12.0]).default_width(420.0).show(ctx, |ui| {
            ui.monospace(format!(
                "frame  p50 {:6.2} ms  p95 {:6.2} ms\nsim    edit {:5.3} ms  tick {:5.3} ms (ema {:5.3})\nticks  {}/frame  total {}  dropped {}  deferred {}\nentities {}",
                percentile(&sorted, 0.5),
                percentile(&sorted, 0.95),
                s.edit_ms_last,
                s.tick_ms_last,
                s.tick_ms_ema,
                s.ticks_last_frame,
                s.ticks_total,
                s.dropped_total,
                s.deferred_total,
                entities.iter().count(),
            ));
        });
    }

    if ui.sim {
        let cal = clock.calendar(config.season_length_days);
        egui::Window::new("Simulation")
            .open(&mut ui.sim)
            .default_pos([12.0, 200.0])
            .show(ctx, |ui| {
                ui.monospace(format!(
                    "tick {}  year {} {:?} day {}  {:02}:{:02}",
                    clock.tick().0,
                    cal.year,
                    cal.season,
                    cal.day_of_season,
                    cal.hour,
                    cal.minute
                ));
                ui.horizontal(|ui| {
                    for (label, value) in [
                        ("Pause", SimSpeed::Paused),
                        ("1×", SimSpeed::X1),
                        ("2×", SimSpeed::X2),
                        ("4×", SimSpeed::X4),
                    ] {
                        if ui.selectable_label(*speed == value, label).clicked() {
                            *speed = value;
                        }
                    }
                    if ui
                        .add_enabled(*speed == SimSpeed::Paused, egui::Button::new("Step"))
                        .clicked()
                    {
                        step.0 += 1;
                    }
                });
                // Sim-affecting options go through `SetSimConfig` (ADR-0055), never direct writes.
                let mut next = config.clone();
                ui.horizontal(|ui| {
                    ui.label("Growth");
                    for (label, mode) in [
                        ("off", GrowthMode::Off),
                        ("debug", GrowthMode::Debug),
                        ("normal", GrowthMode::Normal),
                    ] {
                        if ui.selectable_label(next.growth == mode, label).clicked() {
                            next.growth = mode;
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.checkbox(&mut next.sandbox, "sandbox");
                    ui.checkbox(&mut next.traffic_agents, "traffic agents");
                    ui.label("season days");
                    ui.add(egui::DragValue::new(&mut next.season_length_days).range(1..=28));
                });
                if next != *config {
                    queue.push(PlayerCommand::SetSimConfig(next));
                }
                ui.horizontal(|ui| {
                    ui.label("Day length");
                    for d in DayLength::ALL {
                        if ui
                            .selectable_label(
                                pacing.day_length == d,
                                format!("{} min", d.minutes()),
                            )
                            .clicked()
                        {
                            pacing.day_length = d;
                        }
                    }
                });
            });
    }

    if ui.console {
        let lines = ring.map(|r| r.0.tail(200)).unwrap_or_default();
        egui::Window::new("Log")
            .open(&mut ui.console)
            .default_size([720.0, 320.0])
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        for line in &lines {
                            ui.monospace(line);
                        }
                    });
            });
    }

    if ui.inspector {
        egui::Window::new("Inspector")
            .open(&mut ui.inspector)
            .default_size([420.0, 480.0])
            .show(ctx, |ui| {
                ui.label("Sim resources (read-only). Sim entities appear here from M2.");
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (name, value) in &snapshot.0 {
                        egui::CollapsingHeader::new(name)
                            .default_open(true)
                            .show(ui, |ui| {
                                ui.monospace(value);
                            });
                    }
                });
            });
    }
}
