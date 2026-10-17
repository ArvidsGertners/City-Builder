//! The routing graph (ALGORITHMS §12): links are directed road pieces (one per segment and
//! direction), turn edges connect a link to the links leaving its end node. Stored as CSR so a
//! search touches contiguous memory. Costs are integer milliseconds.

use glam::Vec2;

/// Turn penalties (ms): right, straight, left, U-turn.
pub const TURN_RIGHT_MS: u32 = 2_000;
/// Straight on.
pub const TURN_STRAIGHT_MS: u32 = 0;
/// Left turn.
pub const TURN_LEFT_MS: u32 = 5_000;
/// U-turn (only at dead ends).
pub const TURN_U_MS: u32 = 20_000;
/// Dirt roads cost this much more (class preference).
pub const DIRT_FACTOR: f32 = 1.2;
/// Angle (radians) within which a turn counts as straight on.
const STRAIGHT_RAD: f32 = 0.5;

/// One link as given by the caller.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinkSpec {
    /// Start node index.
    pub from: u32,
    /// End node index.
    pub to: u32,
    /// Length (m).
    pub length_m: f32,
    /// Free-flow speed (m/s).
    pub speed_mps: f32,
    /// Dirt road (class preference).
    pub dirt: bool,
    /// Capacity (veh per sim-hour).
    pub capacity_per_sim_h: f32,
    /// Direction leaving `from` (unit), for turn classification.
    pub dir_out: Vec2,
    /// Direction entering `to` (unit).
    pub dir_in: Vec2,
    /// Caller's key (e.g. segment id and direction), carried through untouched.
    pub key: u64,
}

/// The CSR graph.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Graph {
    links: Vec<LinkSpec>,
    /// Successors of link `l`: `edges[off[l]..off[l + 1]]` = (next link, turn ms).
    off: Vec<u32>,
    edges: Vec<(u32, u32)>,
    /// Predecessors (reverse CSR) for backward landmark distances.
    roff: Vec<u32>,
    redges: Vec<(u32, u32)>,
    /// Links leaving each node.
    node_off: Vec<u32>,
    node_out: Vec<u32>,
    /// Links entering each node.
    node_in_off: Vec<u32>,
    node_in: Vec<u32>,
    node_pos: Vec<Vec2>,
    free_ms: Vec<u32>,
}

fn turn_ms(a: Vec2, b: Vec2) -> u32 {
    let dot = a.dot(b).clamp(-1.0, 1.0);
    let cross = a.perp_dot(b);
    let angle = cb_core::math::atan2(cross, dot);
    if angle.abs() < STRAIGHT_RAD {
        TURN_STRAIGHT_MS
    } else if angle.abs() > core::f32::consts::PI - 0.2 {
        TURN_U_MS
    } else if angle > 0.0 {
        TURN_LEFT_MS
    } else {
        TURN_RIGHT_MS
    }
}

fn csr(n: usize, pairs: &mut [(u32, (u32, u32))]) -> (Vec<u32>, Vec<(u32, u32)>) {
    pairs.sort_unstable_by_key(|p| (p.0, p.1.0));
    let mut off = vec![0u32; n + 1];
    for (k, _) in pairs.iter() {
        off[*k as usize + 1] += 1;
    }
    for i in 0..n {
        off[i + 1] += off[i];
    }
    (off, pairs.iter().map(|p| p.1).collect())
}

impl Graph {
    /// Builds the graph. U-turns are only allowed where a link's end node has no other exit.
    #[must_use]
    pub fn build(node_pos: Vec<Vec2>, links: Vec<LinkSpec>) -> Self {
        let n_nodes = node_pos.len();
        let n = links.len();
        let mut out_pairs: Vec<(u32, (u32, u32))> = links
            .iter()
            .enumerate()
            .map(|(i, l)| (l.from, (i as u32, 0)))
            .collect();
        let (node_off, node_out_pairs) = csr(n_nodes, &mut out_pairs);
        let node_out: Vec<u32> = node_out_pairs.iter().map(|p| p.0).collect();
        let mut in_pairs: Vec<(u32, (u32, u32))> = links
            .iter()
            .enumerate()
            .map(|(i, l)| (l.to, (i as u32, 0)))
            .collect();
        let (node_in_off, node_in_pairs) = csr(n_nodes, &mut in_pairs);
        let node_in: Vec<u32> = node_in_pairs.iter().map(|p| p.0).collect();
        let mut succ: Vec<(u32, (u32, u32))> = Vec::new();
        for (i, l) in links.iter().enumerate() {
            let v = l.to as usize;
            let exits: Vec<u32> = node_out[node_off[v] as usize..node_off[v + 1] as usize].to_vec();
            let non_u: Vec<u32> = exits
                .iter()
                .copied()
                .filter(|m| links[*m as usize].to != l.from)
                .collect();
            let allowed = if non_u.is_empty() { exits } else { non_u };
            for m in allowed {
                let t = turn_ms(l.dir_in, links[m as usize].dir_out);
                succ.push((i as u32, (m, t)));
            }
        }
        let mut pred: Vec<(u32, (u32, u32))> =
            succ.iter().map(|(a, (b, t))| (*b, (*a, *t))).collect();
        let (off, edges) = csr(n, &mut succ);
        let (roff, redges) = csr(n, &mut pred);
        let free_ms = links
            .iter()
            .map(|l| {
                let f = if l.dirt { DIRT_FACTOR } else { 1.0 };
                ((l.length_m / l.speed_mps.max(0.1)) * 1000.0 * f)
                    .round()
                    .max(1.0) as u32
            })
            .collect();
        Self {
            links,
            off,
            edges,
            roff,
            redges,
            node_off,
            node_out,
            node_in_off,
            node_in,
            node_pos,
            free_ms,
        }
    }

    /// Number of links.
    #[must_use]
    pub fn link_count(&self) -> usize {
        self.links.len()
    }

    /// Number of nodes.
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.node_pos.len()
    }

    /// A link.
    #[must_use]
    pub fn link(&self, l: u32) -> &LinkSpec {
        &self.links[l as usize]
    }

    /// Free-flow cost of a link (ms).
    #[must_use]
    pub fn free_ms(&self, l: u32) -> u32 {
        self.free_ms[l as usize]
    }

    /// Free-flow costs of all links.
    #[must_use]
    pub fn free_costs(&self) -> &[u32] {
        &self.free_ms
    }

    /// Successor links with turn penalties.
    #[must_use]
    pub fn succ(&self, l: u32) -> &[(u32, u32)] {
        &self.edges[self.off[l as usize] as usize..self.off[l as usize + 1] as usize]
    }

    /// Predecessor links with turn penalties.
    #[must_use]
    pub fn pred(&self, l: u32) -> &[(u32, u32)] {
        &self.redges[self.roff[l as usize] as usize..self.roff[l as usize + 1] as usize]
    }

    /// Links leaving a node.
    #[must_use]
    pub fn out_links(&self, node: u32) -> &[u32] {
        let v = node as usize;
        &self.node_out[self.node_off[v] as usize..self.node_off[v + 1] as usize]
    }

    /// Links entering a node.
    #[must_use]
    pub fn in_links(&self, node: u32) -> &[u32] {
        let v = node as usize;
        &self.node_in[self.node_in_off[v] as usize..self.node_in_off[v + 1] as usize]
    }

    /// Node position.
    #[must_use]
    pub fn node_pos(&self, node: u32) -> Vec2 {
        self.node_pos[node as usize]
    }
}
