//! Network: nodes, directed links with lanes, Bézier connectors, conflict zones, controls.
//! Right-hand traffic; lane 0 = leftmost lane of a direction.

use glam::Vec2;

pub const LANE_W: f32 = 3.5;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Class {
    Street,
    Avenue,
    Ring,
}

impl Class {
    pub fn v_free(self) -> f32 {
        match self {
            Class::Street => 13.9,
            Class::Avenue => 16.7,
            Class::Ring => 8.3,
        }
    }
    pub fn rank(self) -> u8 {
        match self {
            Class::Street => 0,
            Class::Avenue => 2,
            Class::Ring => 3,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ctrl {
    /// Classified from the road hierarchy.
    Auto,
    Signal,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NodeCtrl {
    None,
    Equal,
    Priority,
    Signal,
    Ring,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Approach {
    Priority,
    Yield,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Turn {
    Right,
    Straight,
    Left,
    U,
}

impl Turn {
    pub fn bit(self) -> u8 {
        match self {
            Turn::Straight => 1,
            Turn::Right => 2,
            Turn::Left => 4,
            Turn::U => 8,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Node {
    pub pos: Vec2,
    pub want: Ctrl,
    pub ctrl: NodeCtrl,
    pub inc: Vec<u32>,
    pub out: Vec<u32>,
    pub cut: f32,
    /// Signal phases: incoming links per phase and the turns they may make (bits: straight 1,
    /// right 2, left 4, U 8). Each axis gets a through+right phase and a protected-left phase.
    pub phases: Vec<Vec<u32>>,
    pub phase_turns: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct Link {
    pub from: u32,
    pub to: u32,
    pub class: Class,
    pub lanes: u8,
    pub lane0: u32,
    pub a: Vec2,
    pub b: Vec2,
    pub dir: Vec2,
    pub len: f32,
    pub v_free: f32,
    pub approach: Approach,
    pub alive: bool,
    pub ring: bool,
}

#[derive(Clone, Debug)]
pub struct Lane {
    pub link: u32,
    pub idx: u8,
    pub len: f32,
    pub p0: Vec2,
    pub p1: Vec2,
    pub conns: Vec<u32>,
}

#[derive(Clone, Debug)]
pub struct Conn {
    pub node: u32,
    pub from_lane: u32,
    pub to_lane: u32,
    pub from_link: u32,
    pub to_link: u32,
    pub len: f32,
    pub pts: Vec<Vec2>,
    /// (zone, s_in, s_out), sorted by s_out.
    pub zones: Vec<(u32, f32, f32)>,
    pub turn: Turn,
    pub v_max: f32,
}

#[derive(Clone, Debug)]
pub struct Zone {
    pub node: u32,
    pub conns: [u32; 2],
}

#[derive(Default)]
pub struct Net {
    pub nodes: Vec<Node>,
    pub links: Vec<Link>,
    pub lanes: Vec<Lane>,
    pub conns: Vec<Conn>,
    pub zones: Vec<Zone>,
    /// Conflicting zone pairs: conns sharing at least one zone (per connector, sorted).
    pub conflicts: Vec<Vec<u32>>,
    pub version: u32,
}

fn cross(a: Vec2, b: Vec2) -> f32 {
    a.x * b.y - a.y * b.x
}

fn seg_x(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> Option<(f32, f32)> {
    let r = b - a;
    let s = d - c;
    let den = cross(r, s);
    if den.abs() < 1e-9 {
        return None;
    }
    let t = cross(c - a, s) / den;
    let u = cross(c - a, r) / den;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then_some((t, u))
}

impl Net {
    pub fn add_node(&mut self, pos: Vec2, want: Ctrl) -> u32 {
        self.nodes.push(Node {
            pos,
            want,
            ctrl: NodeCtrl::None,
            inc: Vec::new(),
            out: Vec::new(),
            cut: 0.0,
            phases: Vec::new(),
            phase_turns: Vec::new(),
        });
        (self.nodes.len() - 1) as u32
    }

    /// Adds one directed link (call twice for two-way roads).
    pub fn add_link(&mut self, from: u32, to: u32, class: Class, lanes: u8) -> u32 {
        let id = self.links.len() as u32;
        self.links.push(Link {
            from,
            to,
            class,
            lanes,
            lane0: 0,
            a: Vec2::ZERO,
            b: Vec2::ZERO,
            dir: Vec2::X,
            len: 0.0,
            v_free: class.v_free(),
            approach: Approach::Priority,
            alive: true,
            ring: class == Class::Ring,
        });
        self.nodes[from as usize].out.push(id);
        self.nodes[to as usize].inc.push(id);
        id
    }

    pub fn add_road(&mut self, a: u32, b: u32, class: Class, lanes_ab: u8, lanes_ba: u8) {
        if lanes_ab > 0 {
            self.add_link(a, b, class, lanes_ab);
        }
        if lanes_ba > 0 {
            self.add_link(b, a, class, lanes_ba);
        }
    }

    fn reverse_of(&self, l: u32) -> Option<u32> {
        let k = &self.links[l as usize];
        self.nodes[k.to as usize]
            .out
            .iter()
            .copied()
            .find(|o| self.links[*o as usize].to == k.from && self.links[*o as usize].alive)
    }

    /// Lateral offset of lane `idx` (right of the centreline for two-way roads).
    fn lane_offset(&self, l: u32, idx: u8) -> f32 {
        let k = &self.links[l as usize];
        let two_way = self.reverse_of(l).is_some();
        let median = if k.class == Class::Avenue && two_way {
            1.0
        } else {
            0.0
        };
        if two_way {
            median + (idx as f32 + 0.5) * LANE_W
        } else {
            (idx as f32 + 0.5 - k.lanes as f32 * 0.5) * LANE_W
        }
    }

    pub fn finalize(&mut self) {
        // Cut-backs (S-J/ADR-0063 style: widest approach half width + 6 m corner allowance).
        for n in 0..self.nodes.len() {
            let node = &self.nodes[n];
            let mut w: f32 = 0.0;
            for &l in node.inc.iter().chain(node.out.iter()) {
                let k = &self.links[l as usize];
                let two = self.reverse_of(l).is_some();
                let width = k.lanes as f32 * LANE_W * if two { 2.0 } else { 1.0 }
                    + if k.class == Class::Avenue { 1.0 } else { 0.0 };
                w = w.max(0.5 * width);
            }
            let deg = node.inc.len().max(node.out.len());
            self.nodes[n].cut = if deg <= 1 {
                0.0
            } else if self.nodes[n]
                .inc
                .iter()
                .any(|l| self.links[*l as usize].ring)
            {
                4.0
            } else {
                w + 4.0
            };
        }
        // Link geometry and lanes.
        self.lanes.clear();
        for l in 0..self.links.len() {
            let (from, to) = (self.links[l].from as usize, self.links[l].to as usize);
            let (pa, pb) = (self.nodes[from].pos, self.nodes[to].pos);
            let dir = (pb - pa).normalize_or(Vec2::X);
            let a = pa + dir * self.nodes[from].cut;
            let b = pb - dir * self.nodes[to].cut;
            let len = (b - a).dot(dir).max(5.0);
            let k = &mut self.links[l];
            k.dir = dir;
            k.a = a;
            k.b = a + dir * len;
            k.len = len;
            k.lane0 = self.lanes.len() as u32;
            let right = Vec2::new(dir.y, -dir.x);
            let lanes = k.lanes;
            for i in 0..lanes {
                let off = self.lane_offset(l as u32, i);
                let k = &self.links[l];
                self.lanes.push(Lane {
                    link: l as u32,
                    idx: i,
                    len,
                    p0: k.a + right * off,
                    p1: k.b + right * off,
                    conns: Vec::new(),
                });
            }
        }
        // Connectors.
        self.conns.clear();
        for n in 0..self.nodes.len() {
            let inc = self.nodes[n].inc.clone();
            let out = self.nodes[n].out.clone();
            for &li in &inc {
                if !self.links[li as usize].alive {
                    continue;
                }
                let lk = self.links[li as usize].clone();
                // Movements with angles; U-turn only at dead ends.
                let mut mv: Vec<(u32, f32)> = out
                    .iter()
                    .copied()
                    .filter(|o| self.links[*o as usize].alive)
                    .filter(|o| {
                        self.links[*o as usize].to != lk.from
                            || out
                                .iter()
                                .filter(|q| self.links[**q as usize].alive)
                                .count()
                                == 1
                    })
                    .map(|o| {
                        let d = self.links[o as usize].dir;
                        (o, cross(lk.dir, d).atan2(lk.dir.dot(d)))
                    })
                    .collect();
                mv.sort_by(|a, b| b.1.total_cmp(&a.1)); // leftmost first
                let n_in = lk.lanes as usize;
                for &(o, ang) in &mv {
                    let ok = self.links[o as usize].clone();
                    let n_out = ok.lanes as usize;
                    let deg = ang.to_degrees();
                    let turn = if ok.to == lk.from {
                        Turn::U
                    } else if deg > 30.0 {
                        Turn::Left
                    } else if deg < -30.0 {
                        Turn::Right
                    } else {
                        Turn::Straight
                    };
                    let is_leftmost = mv.first().is_some_and(|m| m.0 == o) && mv.len() > 1;
                    let is_rightmost = mv.last().is_some_and(|m| m.0 == o) && mv.len() > 1;
                    let pairs: Vec<(usize, usize)> = match turn {
                        _ if n_in == 1 => vec![(
                            0,
                            if turn == Turn::Right {
                                n_out - 1
                            } else if turn == Turn::Left {
                                0
                            } else {
                                0usize.min(n_out - 1)
                            },
                        )],
                        Turn::Left | Turn::U => vec![(0, 0)],
                        Turn::Right => vec![(n_in - 1, n_out - 1)],
                        Turn::Straight => {
                            if is_leftmost {
                                vec![(0, 0)]
                            } else if is_rightmost {
                                vec![(n_in - 1, n_out - 1)]
                            } else {
                                (0..n_in).map(|i| (i, i.min(n_out - 1))).collect()
                            }
                        }
                    };
                    for (fi, ti) in pairs {
                        let from_lane = lk.lane0 + fi as u32;
                        let to_lane = ok.lane0 + ti as u32;
                        let p0 = self.lanes[from_lane as usize].p1;
                        let p1 = self.lanes[to_lane as usize].p0;
                        let d0 = lk.dir;
                        let d1 = ok.dir;
                        let ctrl = match seg_x(p0, p0 + d0 * 200.0, p1 - d1 * 200.0, p1) {
                            Some((t, _)) if turn != Turn::Straight => p0 + d0 * 200.0 * t,
                            _ => (p0 + p1) * 0.5,
                        };
                        let pts: Vec<Vec2> = (0..=10)
                            .map(|q| {
                                let t = q as f32 / 10.0;
                                p0 * (1.0 - t) * (1.0 - t) + ctrl * 2.0 * t * (1.0 - t) + p1 * t * t
                            })
                            .collect();
                        let len: f32 = pts
                            .windows(2)
                            .map(|w| w[0].distance(w[1]))
                            .sum::<f32>()
                            .max(1.0);
                        let v_max = match turn {
                            Turn::Straight => lk.v_free.min(ok.v_free),
                            Turn::Right => 6.0,
                            Turn::Left => 7.5,
                            Turn::U => 3.0,
                        };
                        let id = self.conns.len() as u32;
                        self.conns.push(Conn {
                            node: n as u32,
                            from_lane,
                            to_lane,
                            from_link: li,
                            to_link: o,
                            len,
                            pts,
                            zones: Vec::new(),
                            turn,
                            v_max,
                        });
                        self.lanes[from_lane as usize].conns.push(id);
                    }
                }
            }
        }
        // Zones: crossings between connectors of different incoming lanes; merges into one lane.
        self.zones.clear();
        let mut by_node: Vec<Vec<u32>> = vec![Vec::new(); self.nodes.len()];
        for (c, k) in self.conns.iter().enumerate() {
            by_node[k.node as usize].push(c as u32);
        }
        let arclen = |pts: &[Vec2]| -> Vec<f32> {
            let mut s = vec![0.0];
            for w in pts.windows(2) {
                let v = s[s.len() - 1] + w[0].distance(w[1]);
                s.push(v);
            }
            s
        };
        for (n, cs) in by_node.iter().enumerate() {
            for x in 0..cs.len() {
                for y in x + 1..cs.len() {
                    let (a, b) = (cs[x] as usize, cs[y] as usize);
                    if self.conns[a].from_lane == self.conns[b].from_lane {
                        continue;
                    }
                    let zid = self.zones.len() as u32;
                    if self.conns[a].to_lane == self.conns[b].to_lane {
                        let (la, lb) = (self.conns[a].len, self.conns[b].len);
                        // Merge slot: released once the front has entered the target lane (rear
                        // past len − 4.5); IDM across the connector end keeps the spacing.
                        self.conns[a]
                            .zones
                            .push((zid, (la - 8.0).max(0.0), (la - 4.5).max(0.5)));
                        self.conns[b]
                            .zones
                            .push((zid, (lb - 8.0).max(0.0), (lb - 4.5).max(0.5)));
                        self.zones.push(Zone {
                            node: n as u32,
                            conns: [a as u32, b as u32],
                        });
                        continue;
                    }
                    let (pa, pb) = (self.conns[a].pts.clone(), self.conns[b].pts.clone());
                    let (sa, sb) = (arclen(&pa), arclen(&pb));
                    let mut hit = None;
                    'o: for i in 0..pa.len() - 1 {
                        for j in 0..pb.len() - 1 {
                            if let Some((t, u)) = seg_x(pa[i], pa[i + 1], pb[j], pb[j + 1]) {
                                let da = (pa[i + 1] - pa[i]).normalize_or_zero();
                                let db = (pb[j + 1] - pb[j]).normalize_or_zero();
                                hit = Some((
                                    sa[i] + t * (sa[i + 1] - sa[i]),
                                    sb[j] + u * (sb[j + 1] - sb[j]),
                                    cross(da, db).abs(),
                                ));
                                break 'o;
                            }
                        }
                    }
                    if let Some((s1, s2, sin)) = hit {
                        let half = (2.5 / sin.max(0.2)).clamp(2.5, 9.0);
                        let (la, lb) = (self.conns[a].len, self.conns[b].len);
                        self.conns[a]
                            .zones
                            .push((zid, (s1 - half).max(0.0), (s1 + half).min(la)));
                        self.conns[b]
                            .zones
                            .push((zid, (s2 - half).max(0.0), (s2 + half).min(lb)));
                        self.zones.push(Zone {
                            node: n as u32,
                            conns: [a as u32, b as u32],
                        });
                    }
                }
            }
        }
        for c in &mut self.conns {
            c.zones.sort_by(|a, b| a.2.total_cmp(&b.2));
        }
        self.conflicts = vec![Vec::new(); self.conns.len()];
        for z in &self.zones {
            self.conflicts[z.conns[0] as usize].push(z.conns[1]);
            self.conflicts[z.conns[1] as usize].push(z.conns[0]);
        }
        for c in &mut self.conflicts {
            c.sort_unstable();
            c.dedup();
        }
        self.classify();
        self.version += 1;
    }

    /// Node control from the road hierarchy (ADR-0063 Auto) + signal phases.
    fn classify(&mut self) {
        for n in 0..self.nodes.len() {
            let inc: Vec<u32> = self.nodes[n]
                .inc
                .iter()
                .copied()
                .filter(|l| self.links[*l as usize].alive)
                .collect();
            let outs = self.nodes[n]
                .out
                .iter()
                .filter(|l| self.links[**l as usize].alive)
                .count();
            if inc.iter().any(|l| self.links[*l as usize].ring) {
                self.nodes[n].ctrl = NodeCtrl::Ring;
                for &l in &inc {
                    self.links[l as usize].approach = if self.links[l as usize].ring {
                        Approach::Priority
                    } else {
                        Approach::Yield
                    };
                }
                continue;
            }
            if inc.len() <= 1 || (inc.len() == 2 && outs <= 2) {
                self.nodes[n].ctrl = if inc.len() >= 2 {
                    NodeCtrl::Priority
                } else {
                    NodeCtrl::None
                };
                for &l in &inc {
                    self.links[l as usize].approach = Approach::Priority;
                }
                if inc.len() == 2 {
                    // A merge (2 inflows): the higher class (then lower id) has priority.
                    let (a, b) = (inc[0], inc[1]);
                    let ra = self.links[a as usize].class.rank();
                    let rb = self.links[b as usize].class.rank();
                    let minor = if ra > rb || (ra == rb && a < b) { b } else { a };
                    let both_through = self.links[a as usize].to != self.links[b as usize].from;
                    if both_through && outs == 1 {
                        self.links[minor as usize].approach = Approach::Yield;
                    }
                }
                continue;
            }
            if self.nodes[n].want == Ctrl::Signal {
                self.nodes[n].ctrl = NodeCtrl::Signal;
                // Phases: approaches grouped by axis (opposite directions together).
                let mut phases: Vec<Vec<u32>> = Vec::new();
                for &l in &inc {
                    let d = self.links[l as usize].dir;
                    match phases.iter_mut().find(|p| {
                        let q = self.links[p[0] as usize].dir;
                        d.dot(q).abs() > 0.8
                    }) {
                        Some(p) => p.push(l),
                        None => phases.push(vec![l]),
                    }
                }
                let mut ph = Vec::new();
                let mut pt = Vec::new();
                for g in phases {
                    ph.push(g.clone());
                    pt.push(1 | 2);
                    ph.push(g);
                    pt.push(4 | 8);
                }
                self.nodes[n].phases = ph;
                self.nodes[n].phase_turns = pt;
                for &l in &inc {
                    self.links[l as usize].approach = Approach::Priority;
                }
                continue;
            }
            let top = inc
                .iter()
                .map(|l| self.links[*l as usize].class.rank())
                .max()
                .unwrap_or(0);
            let all_equal = inc
                .iter()
                .all(|l| self.links[*l as usize].class.rank() == top);
            if all_equal {
                self.nodes[n].ctrl = NodeCtrl::Equal;
                for &l in &inc {
                    self.links[l as usize].approach = Approach::Priority;
                }
            } else {
                self.nodes[n].ctrl = NodeCtrl::Priority;
                for &l in &inc {
                    self.links[l as usize].approach = if self.links[l as usize].class.rank() == top
                    {
                        Approach::Priority
                    } else {
                        Approach::Yield
                    };
                }
            }
        }
    }

    /// Incoming link from the right of `l` at its end node (for right-before-left).
    pub fn right_of(&self, l: u32) -> Vec<u32> {
        let k = &self.links[l as usize];
        let n = &self.nodes[k.to as usize];
        n.inc
            .iter()
            .copied()
            .filter(|o| *o != l && self.links[*o as usize].alive)
            .filter(|o| {
                // Vehicles approaching from the right travel in a direction rotated +90° (left) of ours.
                let d = self.links[*o as usize].dir;
                let a = cross(k.dir, d).atan2(k.dir.dot(d)).to_degrees();
                (45.0..=135.0).contains(&a)
            })
            .collect()
    }

    pub fn remove_link(&mut self, l: u32) {
        self.links[l as usize].alive = false;
    }
}
