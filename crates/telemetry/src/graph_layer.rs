#![cfg(feature = "rerun")]

use crate::find_parent_subsystem;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;

#[derive(Debug, Clone)]
#[allow(dead_code)]
enum GraphEvent {
    NodeDiscovered(String),
    NodeStart(String),
    NodeStop(String),
    EdgeActive(String, String),
}

pub struct RerunGraphLayer {
    tx: mpsc::UnboundedSender<GraphEvent>,
}

impl RerunGraphLayer {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let (tx, mut rx) = mpsc::unbounded_channel();

        // Spawn the background task to manage the Rerun graph state
        tokio::spawn(async move {
            let rec = match rerun::RecordingStream::global(rerun::StoreKind::Recording) {
                Some(rec) => rec,
                None => {
                    tracing::warn!("Rerun recording stream not found. Graph Layer won't log.");
                    return;
                }
            };

            let mut nodes: HashSet<String> = HashSet::new();
            let mut edges: HashSet<(String, String)> = HashSet::new();
            let mut active_nodes: HashSet<String> = HashSet::new();
            let mut active_edges: HashMap<(String, String), Instant> = HashMap::new();

            let mut dirty = false;
            let edge_cooldown = Duration::from_millis(400);
            let mut interval = tokio::time::interval(Duration::from_millis(50));

            loop {
                tokio::select! {
                    Some(event) = rx.recv() => {
                        match event {
                            GraphEvent::NodeDiscovered(node) => {
                                if nodes.insert(node) {
                                    dirty = true;
                                }
                            }
                            GraphEvent::NodeStart(node) => {
                                nodes.insert(node.clone());
                                if active_nodes.insert(node) {
                                    dirty = true;
                                }
                            }
                            GraphEvent::NodeStop(node) => {
                                if active_nodes.remove(&node) {
                                    dirty = true;
                                }
                            }
                            GraphEvent::EdgeActive(from, to) => {
                                let edge = (from.clone(), to.clone());
                                nodes.insert(from);
                                nodes.insert(to);
                                edges.insert(edge.clone());
                                active_edges.insert(edge, Instant::now());
                                dirty = true;
                            }
                        }
                    }
                    _ = interval.tick() => {
                        let now = Instant::now();
                        let mut expired = Vec::new();
                        for (edge, time) in &active_edges {
                            if now.duration_since(*time) > edge_cooldown {
                                expired.push(edge.clone());
                            }
                        }
                        for edge in expired {
                            active_edges.remove(&edge);
                            dirty = true;
                        }
                    }
                }

                if dirty {
                    dirty = false;

                    let node_ids: Vec<_> = nodes.iter().cloned().collect();

                    let node_colors: Vec<[u8; 3]> = node_ids
                        .iter()
                        .map(|n| {
                            if active_nodes.contains(n) {
                                [50, 255, 50] // Active Green
                            } else {
                                [100, 100, 100] // Idle Gray
                            }
                        })
                        .collect();

                    let node_radii: Vec<f32> = node_ids.iter().map(|_| 0.5).collect();

                    let graph_nodes = rerun::GraphNodes::new(node_ids.clone())
                        .with_labels(node_ids)
                        .with_colors(node_colors)
                        .with_radii(node_radii);

                    let mut edge_pairs = Vec::new();
                    for edge in &edges {
                        // In rerun 0.31, edges without a visual update just re-render.
                        // We push all known edges.
                        edge_pairs.push((edge.0.clone(), edge.1.clone()));
                    }

                    let graph_edges = rerun::GraphEdges::new(edge_pairs).with_directed_edges();

                    rec.log(
                        "system/architecture",
                        &[
                            &graph_nodes as &dyn rerun::AsComponents,
                            &graph_edges as &dyn rerun::AsComponents,
                        ],
                    )
                    .inspect_err(|e| tracing::error!("{}", e))
                    .ok();
                }
            }
        });

        Self { tx }
    }
}

impl<S> Layer<S> for RerunGraphLayer
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        id: &tracing::span::Id,
        ctx: Context<'_, S>,
    ) {
        if attrs.fields().field("subsystem").is_some() {
            self.tx
                .send(GraphEvent::NodeDiscovered(
                    attrs.metadata().name().to_string(),
                ))
                .inspect_err(|e| tracing::error!("{}", e))
                .ok();
            return;
        }
        if let Some(span) = ctx.span(id)
            && attrs.fields().field("activate_subsystem").is_some()
            && let Some(subsystem_name) = find_parent_subsystem(span)
        {
            self.tx
                .send(GraphEvent::NodeStart(subsystem_name))
                .inspect_err(|e| tracing::error!("{}", e))
                .ok();
        }

        // if let Some(span) = ctx.span(id)
        //     && attrs.fields().field("subsystem").is_some()
        // {
        //     println!(
        //         "Something in subsystem {} {:?}",
        //         span.name(),
        //         span.metadata()
        //     )
        // }

        // if let Some(span) = ctx.span(id)
        //     && let Some(subsystem_name) = find_parent_subsystem(&span)
        // {
        //     println!(
        //         "Something deep in subsystem {} {} {:?}",
        //         subsystem_name,
        //         span.name(),
        //         span.metadata()
        //     )
        // }
    }

    fn on_close(&self, id: tracing::span::Id, ctx: Context<'_, S>) {
        if let Some(span) = ctx.span(&id)
            && span.fields().field("activate_subsystem").is_some()
            && let Some(subsystem_name) = find_parent_subsystem(span)
        {
            self.tx
                .send(GraphEvent::NodeStop(subsystem_name))
                .inspect_err(|e| tracing::error!("{}", e))
                .ok();
        }
    }

    // fn on_event(&self, event: &tracing::Event<'_>, ctx: Context<'_, S>) {
    //     if event.metadata().target() == "channel_send" {
    //         if let Some(span) = ctx.event_span(event)
    //             && let Some(subsystem_name) = find_parent_subsystem(span)
    //         {
    //             println!("Text {:?}", event. event.metadata());
    //             // self.tx
    //             //     .send(GraphEvent::EdgeActive(subsystem_name, to_node.to_string()));
    //         }
    //     }
    // }
}
