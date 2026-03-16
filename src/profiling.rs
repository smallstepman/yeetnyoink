use std::any::Any;
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::Serialize;
use tracing::field::{Field, Visit};
use tracing::{Id, Subscriber};
use tracing_subscriber::layer::{Context as LayerContext, Layer};
use tracing_subscriber::registry::LookupSpan;

#[derive(Debug, Clone)]
pub struct ProfileConfig {
    pub root: PathBuf,
}

impl Default for ProfileConfig {
    fn default() -> Self {
        Self {
            root: PathBuf::from("/tmp/yeetnyoink"),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ArtifactPaths {
    pub run_dir: PathBuf,
    pub chrome_trace: PathBuf,
    pub folded_trace: PathBuf,
    pub flamegraph_svg: PathBuf,
    pub summary_json: PathBuf,
    pub events_json: PathBuf,
    pub manifest_json: PathBuf,
}

impl ArtifactPaths {
    fn create(root: &Path) -> Result<Self> {
        fs::create_dir_all(root)
            .with_context(|| format!("failed to create profiling root {}", root.display()))?;
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let run_dir = root.join(format!("profile-{stamp}-{}", std::process::id()));
        fs::create_dir_all(&run_dir)
            .with_context(|| format!("failed to create profiling dir {}", run_dir.display()))?;

        Ok(Self {
            chrome_trace: run_dir.join("trace.json"),
            folded_trace: run_dir.join("trace.folded"),
            flamegraph_svg: run_dir.join("flamegraph.svg"),
            summary_json: run_dir.join("summary.json"),
            events_json: run_dir.join("events.json"),
            manifest_json: run_dir.join("manifest.json"),
            run_dir,
        })
    }
}

pub(crate) struct ProfilingSession {
    artifacts: ArtifactPaths,
    recorder: Arc<SpanRecorder>,
    chrome_guard: Option<Box<dyn Any>>,
    flame_guard: Option<Box<dyn Any>>,
    argv: Vec<String>,
    started_at_unix_ms: u64,
    finished: bool,
}

impl ProfilingSession {
    pub(crate) fn start(config: &ProfileConfig, argv: Vec<String>) -> Result<Self> {
        let artifacts = ArtifactPaths::create(&config.root)?;
        let recorder = Arc::new(SpanRecorder::new());
        Ok(Self {
            artifacts,
            recorder,
            chrome_guard: None,
            flame_guard: None,
            argv,
            started_at_unix_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            finished: false,
        })
    }

    pub(crate) fn chrome_layer<S>(&mut self) -> tracing_chrome::ChromeLayer<S>
    where
        S: Subscriber + for<'lookup> LookupSpan<'lookup> + Send + Sync + 'static,
    {
        let (layer, guard) = tracing_chrome::ChromeLayerBuilder::new()
            .file(&self.artifacts.chrome_trace)
            .build();
        self.chrome_guard = Some(Box::new(guard));
        layer
    }

    pub(crate) fn flame_layer<S>(&mut self) -> Result<tracing_flame::FlameLayer<S, BufWriter<File>>>
    where
        S: Subscriber + for<'lookup> LookupSpan<'lookup> + Send + Sync + 'static,
    {
        let (layer, guard) = tracing_flame::FlameLayer::with_file(&self.artifacts.folded_trace)
            .context("failed to create folded tracing output")?;
        self.flame_guard = Some(Box::new(guard));
        Ok(layer)
    }

    pub(crate) fn summary_layer(&self) -> SummaryLayer {
        SummaryLayer::new(self.recorder.clone())
    }

    pub(crate) fn artifact_dir(&self) -> &Path {
        &self.artifacts.run_dir
    }

    pub(crate) fn finish(&mut self) -> Result<()> {
        if self.finished {
            return Ok(());
        }
        self.finished = true;

        self.chrome_guard.take();
        self.flame_guard.take();

        let snapshot = self.recorder.snapshot();
        let report = SummaryReport::from_snapshot(&snapshot, &self.argv, &self.artifacts);
        write_json(&self.artifacts.events_json, &snapshot.events)?;
        write_json(&self.artifacts.summary_json, &report)?;
        self.write_flamegraph()?;
        write_json(
            &self.artifacts.manifest_json,
            &Manifest {
                argv: self.argv.clone(),
                started_at_unix_ms: self.started_at_unix_ms,
                finished_at_unix_ms: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64,
                total_runtime_ns: snapshot.total_runtime_ns,
                artifacts: self.artifacts.clone(),
            },
        )?;
        Ok(())
    }

    fn write_flamegraph(&self) -> Result<()> {
        let folded = File::open(&self.artifacts.folded_trace).with_context(|| {
            format!(
                "failed to open folded profile {}",
                self.artifacts.folded_trace.display()
            )
        })?;
        let output = File::create(&self.artifacts.flamegraph_svg).with_context(|| {
            format!(
                "failed to create flamegraph {}",
                self.artifacts.flamegraph_svg.display()
            )
        })?;
        let mut options = inferno::flamegraph::Options::default();
        options.count_name = "samples".to_string();
        inferno::flamegraph::from_reader(
            &mut options,
            BufReader::new(folded),
            BufWriter::new(output),
        )
        .context("failed to render flamegraph svg")
    }
}

impl Drop for ProfilingSession {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

#[derive(Debug, Clone, Serialize)]
struct Manifest {
    argv: Vec<String>,
    started_at_unix_ms: u64,
    finished_at_unix_ms: u64,
    total_runtime_ns: u64,
    artifacts: ArtifactPaths,
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let file =
        File::create(path).with_context(|| format!("failed to create {}", path.display()))?;
    serde_json::to_writer_pretty(BufWriter::new(file), value)
        .with_context(|| format!("failed to write {}", path.display()))
}

#[derive(Debug, Clone, Serialize)]
pub struct CompletedSpanEvent {
    pub name: String,
    pub target: String,
    pub fields: BTreeMap<String, String>,
    pub thread: String,
    pub start_ns: u64,
    pub end_ns: u64,
    pub duration_ns: u64,
}

#[derive(Debug, Clone)]
struct SpanTemplate {
    name: String,
    target: String,
    fields: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
struct ActiveSpan {
    thread: String,
    start_ns: u64,
}

#[derive(Debug, Clone)]
struct SpanState {
    template: SpanTemplate,
    active: Vec<ActiveSpan>,
}

impl SpanState {
    fn new(template: SpanTemplate) -> Self {
        Self {
            template,
            active: Vec::new(),
        }
    }
}

#[derive(Default)]
struct FieldVisitor {
    fields: BTreeMap<String, String>,
}

impl Visit for FieldVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.fields
            .insert(field.name().to_string(), format!("{value:?}"));
    }
}

#[derive(Default)]
struct RecorderState {
    events: Vec<CompletedSpanEvent>,
}

struct SpanRecorder {
    started_at: Instant,
    state: Mutex<RecorderState>,
}

impl SpanRecorder {
    fn new() -> Self {
        Self {
            started_at: Instant::now(),
            state: Mutex::new(RecorderState::default()),
        }
    }

    fn now_ns(&self) -> u64 {
        self.started_at.elapsed().as_nanos() as u64
    }

    fn record(&self, event: CompletedSpanEvent) {
        if let Ok(mut state) = self.state.lock() {
            state.events.push(event);
        }
    }

    fn snapshot(&self) -> RecorderSnapshot {
        let state = self.state.lock().expect("span recorder lock poisoned");
        RecorderSnapshot {
            events: state.events.clone(),
            total_runtime_ns: self.now_ns(),
        }
    }
}

#[derive(Debug, Clone)]
struct RecorderSnapshot {
    events: Vec<CompletedSpanEvent>,
    total_runtime_ns: u64,
}

#[derive(Debug, Clone, Serialize)]
struct SummaryAggregate {
    name: String,
    target: String,
    fields: BTreeMap<String, String>,
    count: u64,
    total_ns: u64,
    avg_ns: u64,
    max_ns: u64,
}

#[derive(Debug, Clone, Serialize)]
struct SummaryReport {
    argv: Vec<String>,
    artifact_dir: PathBuf,
    event_count: usize,
    total_runtime_ns: u64,
    summary_by_name: Vec<SummaryAggregate>,
    summary_by_label: Vec<SummaryAggregate>,
}

impl SummaryReport {
    fn from_snapshot(
        snapshot: &RecorderSnapshot,
        argv: &[String],
        artifacts: &ArtifactPaths,
    ) -> Self {
        let mut by_name: BTreeMap<(String, String), SummaryAggregate> = BTreeMap::new();
        let mut by_label: BTreeMap<(String, String, BTreeMap<String, String>), SummaryAggregate> =
            BTreeMap::new();

        for event in &snapshot.events {
            let by_name_key = (event.name.clone(), event.target.clone());
            let name_entry = by_name
                .entry(by_name_key)
                .or_insert_with(|| SummaryAggregate {
                    name: event.name.clone(),
                    target: event.target.clone(),
                    fields: BTreeMap::new(),
                    count: 0,
                    total_ns: 0,
                    avg_ns: 0,
                    max_ns: 0,
                });
            accumulate(name_entry, event.duration_ns);

            let by_label_key = (
                event.name.clone(),
                event.target.clone(),
                event.fields.clone(),
            );
            let label_entry = by_label
                .entry(by_label_key)
                .or_insert_with(|| SummaryAggregate {
                    name: event.name.clone(),
                    target: event.target.clone(),
                    fields: event.fields.clone(),
                    count: 0,
                    total_ns: 0,
                    avg_ns: 0,
                    max_ns: 0,
                });
            accumulate(label_entry, event.duration_ns);
        }

        let mut summary_by_name: Vec<_> = by_name.into_values().collect();
        let mut summary_by_label: Vec<_> = by_label.into_values().collect();
        finalize_aggregates(&mut summary_by_name);
        finalize_aggregates(&mut summary_by_label);

        Self {
            argv: argv.to_vec(),
            artifact_dir: artifacts.run_dir.clone(),
            event_count: snapshot.events.len(),
            total_runtime_ns: snapshot.total_runtime_ns,
            summary_by_name,
            summary_by_label,
        }
    }
}

fn accumulate(aggregate: &mut SummaryAggregate, duration_ns: u64) {
    aggregate.count += 1;
    aggregate.total_ns += duration_ns;
    aggregate.max_ns = aggregate.max_ns.max(duration_ns);
}

fn finalize_aggregates(aggregates: &mut [SummaryAggregate]) {
    for aggregate in aggregates.iter_mut() {
        aggregate.avg_ns = if aggregate.count == 0 {
            0
        } else {
            aggregate.total_ns / aggregate.count
        };
    }
    aggregates.sort_by(|left, right| {
        right
            .total_ns
            .cmp(&left.total_ns)
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.target.cmp(&right.target))
    });
}

pub(crate) struct SummaryLayer {
    recorder: Arc<SpanRecorder>,
}

impl SummaryLayer {
    fn new(recorder: Arc<SpanRecorder>) -> Self {
        Self { recorder }
    }
}

impl<S> Layer<S> for SummaryLayer
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        id: &Id,
        ctx: LayerContext<'_, S>,
    ) {
        let Some(span) = ctx.span(id) else {
            return;
        };

        let mut visitor = FieldVisitor::default();
        attrs.record(&mut visitor);
        let metadata = span.metadata();
        span.extensions_mut().insert(SpanState::new(SpanTemplate {
            name: metadata.name().to_string(),
            target: metadata.target().to_string(),
            fields: visitor.fields,
        }));
    }

    fn on_enter(&self, id: &Id, ctx: LayerContext<'_, S>) {
        let Some(span) = ctx.span(id) else {
            return;
        };
        let mut extensions = span.extensions_mut();
        let Some(state) = extensions.get_mut::<SpanState>() else {
            return;
        };
        state.active.push(ActiveSpan {
            thread: current_thread_label(),
            start_ns: self.recorder.now_ns(),
        });
    }

    fn on_exit(&self, id: &Id, ctx: LayerContext<'_, S>) {
        let Some(span) = ctx.span(id) else {
            return;
        };
        let mut extensions = span.extensions_mut();
        let Some(state) = extensions.get_mut::<SpanState>() else {
            return;
        };
        let Some(active) = state.active.pop() else {
            return;
        };

        let end_ns = self.recorder.now_ns();
        self.recorder.record(CompletedSpanEvent {
            name: state.template.name.clone(),
            target: state.template.target.clone(),
            fields: state.template.fields.clone(),
            thread: active.thread,
            start_ns: active.start_ns,
            end_ns,
            duration_ns: end_ns.saturating_sub(active.start_ns),
        });
    }
}

fn current_thread_label() -> String {
    let current = std::thread::current();
    match current.name() {
        Some(name) => format!("{} ({:?})", name, current.id()),
        None => format!("{:?}", current.id()),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        accumulate, finalize_aggregates, ArtifactPaths, CompletedSpanEvent, RecorderSnapshot,
        SummaryAggregate, SummaryReport,
    };
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use std::time::Duration;
    use tracing_subscriber::prelude::*;

    #[test]
    fn artifact_paths_live_under_requested_root() {
        let root = std::env::temp_dir().join("yeetnyoink-profile-tests");
        let artifacts = ArtifactPaths::create(&root).expect("artifact paths should be created");
        assert!(artifacts.run_dir.starts_with(&root));
        assert_eq!(
            artifacts.chrome_trace.parent(),
            Some(artifacts.run_dir.as_path())
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn summary_report_aggregates_by_name_and_label() {
        let snapshot = RecorderSnapshot {
            events: vec![
                CompletedSpanEvent {
                    name: "vscode.request_value".to_string(),
                    target: "yeetnyoink::adapters::apps::vscode".to_string(),
                    fields: BTreeMap::from([("command".to_string(), "layout".to_string())]),
                    thread: "main".to_string(),
                    start_ns: 0,
                    end_ns: 10,
                    duration_ns: 10,
                },
                CompletedSpanEvent {
                    name: "vscode.request_value".to_string(),
                    target: "yeetnyoink::adapters::apps::vscode".to_string(),
                    fields: BTreeMap::from([("command".to_string(), "focus".to_string())]),
                    thread: "main".to_string(),
                    start_ns: 11,
                    end_ns: 41,
                    duration_ns: 30,
                },
            ],
            total_runtime_ns: 50,
        };
        let artifacts = ArtifactPaths {
            run_dir: PathBuf::from("/tmp/yeetnyoink/test"),
            chrome_trace: PathBuf::from("trace.json"),
            folded_trace: PathBuf::from("trace.folded"),
            flamegraph_svg: PathBuf::from("flamegraph.svg"),
            summary_json: PathBuf::from("summary.json"),
            events_json: PathBuf::from("events.json"),
            manifest_json: PathBuf::from("manifest.json"),
        };
        let report = SummaryReport::from_snapshot(&snapshot, &["yny".into()], &artifacts);
        assert_eq!(report.event_count, 2);
        assert_eq!(report.summary_by_name.len(), 1);
        assert_eq!(report.summary_by_name[0].total_ns, 40);
        assert_eq!(report.summary_by_label.len(), 2);
        assert_eq!(report.summary_by_label[0].max_ns, 30);
    }

    #[test]
    fn aggregates_compute_average_and_sort_descending() {
        let mut aggregates = vec![
            SummaryAggregate {
                name: "b".into(),
                target: "t".into(),
                fields: BTreeMap::new(),
                count: 2,
                total_ns: 20,
                avg_ns: 0,
                max_ns: 15,
            },
            SummaryAggregate {
                name: "a".into(),
                target: "t".into(),
                fields: BTreeMap::new(),
                count: 1,
                total_ns: 50,
                avg_ns: 0,
                max_ns: 50,
            },
        ];
        finalize_aggregates(&mut aggregates);
        assert_eq!(aggregates[0].name, "a");
        assert_eq!(aggregates[0].avg_ns, 50);
        assert_eq!(aggregates[1].avg_ns, 10);
    }

    #[test]
    fn accumulate_updates_count_total_and_max() {
        let mut aggregate = SummaryAggregate {
            name: "span".into(),
            target: "target".into(),
            fields: BTreeMap::new(),
            count: 0,
            total_ns: 0,
            avg_ns: 0,
            max_ns: 0,
        };
        accumulate(&mut aggregate, 5);
        accumulate(&mut aggregate, 8);
        assert_eq!(aggregate.count, 2);
        assert_eq!(aggregate.total_ns, 13);
        assert_eq!(aggregate.max_ns, 8);
    }

    #[test]
    fn profiling_session_writes_trace_summary_and_flamegraph_artifacts() {
        let root = std::env::temp_dir().join("yeetnyoink-profile-run-tests");
        let mut session = super::ProfilingSession::start(
            &super::ProfileConfig { root: root.clone() },
            vec![
                "yny".into(),
                "--profile".into(),
                "focus".into(),
                "west".into(),
            ],
        )
        .expect("profiling session should start");
        let chrome = session.chrome_layer();
        let flame = session.flame_layer().expect("flame layer should build");
        let summary = session.summary_layer();
        let subscriber = tracing_subscriber::registry()
            .with(chrome)
            .with(flame)
            .with(summary);

        tracing::subscriber::with_default(subscriber, || {
            let span = tracing::debug_span!("profiling.test_span", step = "inner");
            let _entered = span.enter();
            std::thread::sleep(Duration::from_millis(2));
        });

        session.finish().expect("profiling session should finish");
        let artifact_dir = session.artifact_dir().to_path_buf();
        assert!(artifact_dir.join("trace.json").exists());
        assert!(artifact_dir.join("trace.folded").exists());
        assert!(artifact_dir.join("flamegraph.svg").exists());
        assert!(artifact_dir.join("summary.json").exists());
        assert!(artifact_dir.join("events.json").exists());

        let summary = std::fs::read_to_string(artifact_dir.join("summary.json"))
            .expect("summary should be readable");
        assert!(summary.contains("profiling.test_span"));

        let _ = std::fs::remove_dir_all(root);
    }
}
