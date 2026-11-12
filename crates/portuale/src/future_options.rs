//! Future options (#706): behaviour that is not a plain option yet, switched
//! on by name with `--future-options=name,other=value`. Each one is in
//! [`GATES`] with its stage, so promoting it to a plain option or removing it
//! is one line there. `alpha` is off unless asked for, `beta` is on unless
//! switched off with `name=false`, `ga` is always on and stays only so old
//! command lines still parse.
//!
//! Precedence per option, lowest to highest: the stage default, `make.conf`,
//! the environment (`PORTUALE_FUTURE_OPTIONS`), the command line. When the
//! command line carries the option, `make.conf` is not read at all.

use std::collections::BTreeMap;

/// The variable the environment and `make.conf` carry, same syntax as the option.
pub(crate) const VAR: &str = "PORTUALE_FUTURE_OPTIONS";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[allow(dead_code, reason = "no option has been promoted to beta or ga yet")]
pub(crate) enum Stage {
    Alpha,
    Beta,
    Ga,
}

/// One registered option.
pub(crate) struct Gate {
    pub name: &'static str,
    /// A switch (`name`, `name=false`) or a value (`name=N`).
    pub takes_value: bool,
    pub stage: Stage,
    /// The release that introduced it.
    pub since: &'static str,
    pub help: &'static str,
}

/// Every future option; add or delete a line here, nothing else lists them.
pub(crate) const GATES: &[Gate] = &[
    Gate {
        name: "check-world-unique",
        takes_value: false,
        stage: Stage::Alpha,
        since: "300.83",
        help: "--check-world: a slot with several satisfying versions is unsatisfiable",
    },
    Gate {
        name: "check-world-threads",
        takes_value: true,
        stage: Stage::Alpha,
        since: "300.83",
        help: "--check-world: number of threads (default: every core)",
    },
    Gate {
        name: "check-world-unneeded",
        takes_value: false,
        stage: Stage::Alpha,
        since: "300.83",
        help: "--check-world: also list the slots another slot serves",
    },
    Gate {
        name: "plan-world-newuse",
        takes_value: false,
        stage: Stage::Alpha,
        since: "300.83",
        help: "--plan-world: rebuild a kept package whose installed USE differs from the configured one",
    },
];

/// The registry as text for `mrg --help`: name, stage, release, what it does.
pub(crate) fn describe() -> String {
    let mut out = String::from("Future options (--future-options=name,other=value):\n");
    for g in GATES {
        let stage = format!("{:?}", g.stage).to_lowercase();
        out += &format!("  {:<24} {stage} since {}: {}\n", g.name, g.since, g.help);
    }
    out
}

/// The settled value of every registered option.
pub(crate) struct FutureOptions(BTreeMap<&'static str, String>);

impl FutureOptions {
    /// Whether a switch is on. Only the registered names exist.
    pub(crate) fn on(&self, name: &str) -> bool {
        self.0.get(name).is_some_and(|v| v == "true")
    }

    /// The value of a value option, `None` when nobody set it.
    pub(crate) fn value(&self, name: &str) -> Option<&str> {
        self.0
            .get(name)
            .map(String::as_str)
            .filter(|v| !v.is_empty())
    }
}

/// The stage default: a switch is on from `beta` up, a value is unset.
fn default_of(gate: &Gate) -> String {
    match (gate.takes_value, gate.stage) {
        (false, Stage::Beta | Stage::Ga) => "true".into(),
        (false, Stage::Alpha) => "false".into(),
        (true, _) => String::new(),
    }
}

/// `name,other=value` into `(gate, value)` pairs. An unknown name is refused
/// with the registered names closest to it.
fn parse(spec: &str, origin: &str) -> Result<Vec<(&'static Gate, String)>, String> {
    let mut out = Vec::new();
    for item in spec.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let (name, value) = item
            .split_once('=')
            .map_or((item, None), |(n, v)| (n, Some(v)));
        let Some(gate) = GATES.iter().find(|g| g.name == name) else {
            return Err(format!(
                "{origin}: unknown future option {name:?}; closest: {}",
                nearest(name).join(", ")
            ));
        };
        let value = match (gate.takes_value, value) {
            (false, None) => "true".to_string(),
            (false, Some(v @ ("true" | "false"))) => v.to_string(),
            (false, Some(v)) => {
                return Err(format!(
                    "{origin}: {name} is a switch: {name} or {name}=false, not {v:?}"
                ));
            }
            (true, Some(v)) if !v.is_empty() => v.to_string(),
            (true, _) => return Err(format!("{origin}: {name} needs a value: {name}=<value>")),
        };
        out.push((gate, value));
    }
    Ok(out)
}

/// The three registered names closest to `name`.
fn nearest(name: &str) -> Vec<&'static str> {
    let mut names: Vec<(usize, &'static str)> = GATES
        .iter()
        .map(|g| (distance(name, g.name), g.name))
        .collect();
    names.sort();
    names.into_iter().take(3).map(|(_, n)| n).collect()
}

/// Edit distance between two names.
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut diag = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let next = (row[j + 1] + 1)
                .min(row[j] + 1)
                .min(diag + usize::from(ca != *cb));
            diag = row[j + 1];
            row[j + 1] = next;
        }
    }
    row[b.len()]
}

/// Settle every option from the command line `flag`, the environment `env` and
/// `make.conf` (`conf`, read only when the command line says nothing).
///
/// # Errors
/// An unknown name, or a switch or value of the wrong shape, from any source.
pub(crate) fn resolve(
    flag: Option<&str>,
    env: &dyn Fn(&str) -> Option<String>,
    conf: &mut dyn FnMut(&str) -> Option<String>,
) -> Result<FutureOptions, String> {
    let mut settled: BTreeMap<&'static str, String> =
        GATES.iter().map(|g| (g.name, default_of(g))).collect();
    let mut layers: Vec<(Option<String>, &str)> = Vec::new();
    if flag.is_none() {
        layers.push((conf(VAR), "PORTUALE_FUTURE_OPTIONS (make.conf)"));
    }
    layers.push((env(VAR), "PORTUALE_FUTURE_OPTIONS (environment)"));
    layers.push((flag.map(str::to_string), "--future-options"));
    for (spec, origin) in layers {
        for (gate, value) in parse(spec.as_deref().unwrap_or(""), origin)? {
            settled.insert(gate.name, value);
        }
    }
    Ok(FutureOptions(settled))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn registered_names_are_unique_and_documented() {
        for (i, g) in GATES.iter().enumerate() {
            assert!(!g.since.is_empty() && !g.help.is_empty(), "{}", g.name);
            assert!(
                GATES[i + 1..].iter().all(|o| o.name != g.name),
                "{}",
                g.name
            );
        }
    }

    #[test]
    fn stage_defaults() {
        let o = resolve(None, &none, &mut none).unwrap();
        assert!(!o.on("check-world-unique"));
        assert_eq!(o.value("check-world-threads"), None);
    }

    #[test]
    fn a_switch_and_a_value_are_read_and_a_switch_turns_off() {
        let o = resolve(
            Some("check-world-unique,check-world-threads=4"),
            &none,
            &mut none,
        )
        .unwrap();
        assert!(o.on("check-world-unique"));
        assert_eq!(o.value("check-world-threads"), Some("4"));
        let o = resolve(
            Some("check-world-unique=false"),
            &|_| Some("check-world-unique".into()),
            &mut none,
        )
        .unwrap();
        assert!(!o.on("check-world-unique"));
    }

    #[test]
    fn an_unknown_name_names_the_nearest() {
        let e = resolve(Some("check-world-uniqe"), &none, &mut none)
            .err()
            .unwrap();
        assert!(e.contains("closest: check-world-unique"), "{e}");
    }

    #[test]
    fn a_switch_refuses_a_value_and_a_value_needs_one() {
        assert!(resolve(Some("check-world-unique=3"), &none, &mut none).is_err());
        assert!(resolve(Some("check-world-threads"), &none, &mut none).is_err());
    }

    /// Precedence: default < make.conf < environment < command line, per option,
    /// and the command line leaves make.conf unread.
    #[test]
    fn precedence_and_make_conf_is_not_read_under_the_flag() {
        let conf_says = |_: &str| Some("check-world-threads=1,check-world-unique".to_string());
        let env_says = |_: &str| Some("check-world-threads=2".to_string());
        let o = resolve(None, &env_says, &mut { conf_says }).unwrap();
        assert_eq!(o.value("check-world-threads"), Some("2"));
        assert!(o.on("check-world-unique"));
        let mut read = false;
        let mut conf = |_: &str| {
            read = true;
            conf_says("")
        };
        let o = resolve(Some("check-world-threads=3"), &env_says, &mut conf).unwrap();
        assert_eq!(o.value("check-world-threads"), Some("3"));
        assert!(!o.on("check-world-unique"));
        assert!(!read, "make.conf was read although the flag was given");
    }
}
