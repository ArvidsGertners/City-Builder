//! Strongly connected components of the directed node graph (ALGORITHMS §14): buildings whose
//! access node lies outside the main component cannot reach the rest of the city (one-way
//! traps) and are flagged `NoRoute`.

use crate::graph::Graph;

/// Component index per node (iterative Tarjan; components numbered in completion order).
#[must_use]
pub fn components(g: &Graph) -> Vec<u32> {
    let n = g.node_count();
    let mut index = vec![u32::MAX; n];
    let mut low = vec![0u32; n];
    let mut on_stack = vec![false; n];
    let mut comp = vec![u32::MAX; n];
    let mut stack: Vec<u32> = Vec::new();
    let mut next_index = 0u32;
    let mut next_comp = 0u32;
    for root in 0..n as u32 {
        if index[root as usize] != u32::MAX {
            continue;
        }
        // (node, position in its out-link list)
        let mut work: Vec<(u32, usize)> = vec![(root, 0)];
        index[root as usize] = next_index;
        low[root as usize] = next_index;
        next_index += 1;
        stack.push(root);
        on_stack[root as usize] = true;
        while let Some(&mut (v, ref mut pos)) = work.last_mut() {
            let outs = g.out_links(v);
            if *pos < outs.len() {
                let w = g.link(outs[*pos]).to;
                *pos += 1;
                if index[w as usize] == u32::MAX {
                    index[w as usize] = next_index;
                    low[w as usize] = next_index;
                    next_index += 1;
                    stack.push(w);
                    on_stack[w as usize] = true;
                    work.push((w, 0));
                } else if on_stack[w as usize] {
                    low[v as usize] = low[v as usize].min(index[w as usize]);
                }
            } else {
                work.pop();
                if let Some(&(parent, _)) = work.last() {
                    low[parent as usize] = low[parent as usize].min(low[v as usize]);
                }
                if low[v as usize] == index[v as usize] {
                    while let Some(w) = stack.pop() {
                        on_stack[w as usize] = false;
                        comp[w as usize] = next_comp;
                        if w == v {
                            break;
                        }
                    }
                    next_comp += 1;
                }
            }
        }
    }
    comp
}

/// Nodes outside the largest component (ties: the component of the smallest node), excluding
/// isolated nodes without links.
#[must_use]
pub fn trapped(g: &Graph) -> Vec<u32> {
    let comp = components(g);
    let mut size: std::collections::BTreeMap<u32, (usize, u32)> = std::collections::BTreeMap::new();
    for (v, c) in comp.iter().enumerate() {
        let e = size.entry(*c).or_insert((0, v as u32));
        e.0 += 1;
    }
    let main = size
        .iter()
        .max_by(|a, b| a.1.0.cmp(&b.1.0).then(b.1.1.cmp(&a.1.1)))
        .map(|(c, _)| *c);
    (0..g.node_count() as u32)
        .filter(|v| {
            Some(comp[*v as usize]) != main
                && !(g.out_links(*v).is_empty() && g.in_links(*v).is_empty())
        })
        .collect()
}
