use std::collections::{BTreeMap, VecDeque};

use crucible_ownership_qualification::{
    DeferredFreshness, DeferredId, DeferredWork, DomainId, EffectId, EffectPayload, OwnershipError,
    OwnershipSimulator, SemanticDigest, WorkerId,
};

const DOMAIN_COUNT: u16 = 8;
const STAGE_COUNT: u64 = 64;

#[derive(Clone, Copy, Debug)]
enum Op {
    Add(i64),
    AddStageRead {
        source: DomainId,
        bias: i64,
    },
    PrepareSameGeneration {
        id: DeferredId,
        delta: i64,
    },
    Install {
        id: DeferredId,
    },
    EmitAdd {
        id: EffectId,
        target: DomainId,
        delta: i64,
    },
    EmitRaise {
        id: EffectId,
        target: DomainId,
        source: DomainId,
        bias: i64,
    },
}

#[derive(Debug)]
struct StagePlan {
    ops: BTreeMap<DomainId, Vec<Op>>,
    migrate: Vec<DomainId>,
}

#[derive(Debug)]
struct Program {
    initial_values: BTreeMap<DomainId, i64>,
    stages: Vec<StagePlan>,
}

#[derive(Clone, Copy, Debug)]
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut value = self.0;
        value ^= value << 13;
        value ^= value >> 7;
        value ^= value << 17;
        self.0 = value;
        value
    }

    fn index(&mut self, upper: usize) -> usize {
        let upper = u64::try_from(upper).expect("small scheduler candidate count fits u64");
        usize::try_from(self.next() % upper).expect("bounded scheduler index fits usize")
    }
}

fn build_program() -> Program {
    let initial_values = (0..DOMAIN_COUNT)
        .map(|domain| {
            let domain = DomainId(domain);
            (domain, i64::from(domain.0) * 17 - 31)
        })
        .collect();

    let mut deferred_id = 1_u64;
    let mut effect_id = 1_u64;
    let mut stages = Vec::new();
    for stage in 0..STAGE_COUNT {
        let mut ops = BTreeMap::new();
        for raw_domain in 0..DOMAIN_COUNT {
            let domain = DomainId(raw_domain);
            let neighbor = DomainId((raw_domain + 1) % DOMAIN_COUNT);
            let second_neighbor = DomainId((raw_domain + 2) % DOMAIN_COUNT);
            let stage_i64 = i64::try_from(stage).expect("test stage fits i64");
            let local_delta = (stage_i64 + i64::from(raw_domain) * 3).rem_euclid(11) - 5;
            let work_id = DeferredId(deferred_id);
            deferred_id += 1;
            let add_effect = EffectId(effect_id);
            effect_id += 1;
            let raise_effect = EffectId(effect_id);
            effect_id += 1;

            ops.insert(
                domain,
                vec![
                    Op::Add(local_delta),
                    Op::AddStageRead {
                        source: neighbor,
                        bias: i64::from(raw_domain).rem_euclid(5) - 2,
                    },
                    Op::PrepareSameGeneration {
                        id: work_id,
                        delta: (stage_i64 + i64::from(raw_domain)).rem_euclid(7) - 3,
                    },
                    // Deliberately advance revision between prepare/install. Same-generation work
                    // must survive this but will still die across a later migration.
                    Op::Add(1),
                    Op::Install { id: work_id },
                    Op::EmitAdd {
                        id: add_effect,
                        target: neighbor,
                        delta: i64::from(raw_domain).rem_euclid(5) - 2,
                    },
                    Op::EmitRaise {
                        id: raise_effect,
                        target: second_neighbor,
                        source: domain,
                        bias: stage_i64.rem_euclid(3) - 1,
                    },
                ],
            );
        }

        let migrate = (0..DOMAIN_COUNT)
            .map(DomainId)
            .filter(|domain| (u64::from(domain.0) + stage) % 3 == 0)
            .collect();
        stages.push(StagePlan { ops, migrate });
    }
    Program {
        initial_values,
        stages,
    }
}

fn execute_op(
    simulator: &mut OwnershipSimulator,
    domain: DomainId,
    op: Op,
    deferred: &mut BTreeMap<DeferredId, DeferredWork>,
) -> Result<(), OwnershipError> {
    let token = simulator.token(domain)?;
    match op {
        Op::Add(delta) => {
            simulator.mutate(token, delta)?;
        }
        Op::AddStageRead { source, bias } => {
            let observed = simulator.stage_value(source)?;
            // Keep long-trace values bounded while still making every domain depend on another
            // domain's stage image. The value is stable regardless of worker interleaving.
            let delta = observed.rem_euclid(13) - 6 + bias;
            simulator.mutate(token, delta)?;
        }
        Op::PrepareSameGeneration { id, delta } => {
            let work =
                simulator.prepare_deferred(token, id, delta, DeferredFreshness::SameGeneration)?;
            assert!(deferred.insert(id, work).is_none(), "unique deferred ID");
        }
        Op::Install { id } => {
            let work = deferred
                .remove(&id)
                .expect("program prepares before install");
            simulator.install_deferred(token, work)?;
        }
        Op::EmitAdd { id, target, delta } => {
            simulator.emit_effect(token, id, target, EffectPayload::Add(delta))?;
        }
        Op::EmitRaise {
            id,
            target,
            source,
            bias,
        } => {
            let floor = simulator.stage_value(source)?.rem_euclid(97) - 48 + bias;
            simulator.emit_effect(token, id, target, EffectPayload::RaiseToAtLeast(floor))?;
        }
    }
    Ok(())
}

fn run(program: &Program, workers: u16, scheduler_seed: u64) -> Vec<SemanticDigest> {
    assert!(workers > 0);
    let initial = program
        .initial_values
        .iter()
        .map(|(&domain, &value)| (domain, WorkerId(domain.0 % workers), value));
    let mut simulator = OwnershipSimulator::new(initial).expect("valid generated program");
    let mut rng = Rng(scheduler_seed);
    let mut deferred = BTreeMap::new();
    let mut stage_digests = Vec::with_capacity(program.stages.len());

    for (stage_index, stage) in program.stages.iter().enumerate() {
        if stage_index != 0 {
            simulator
                .begin_stage()
                .expect("next stage opens after migration");
        }

        let mut queues: BTreeMap<DomainId, VecDeque<Op>> = stage
            .ops
            .iter()
            .map(|(&domain, ops)| (domain, ops.iter().copied().collect()))
            .collect();
        loop {
            let ready: Vec<_> = queues
                .iter()
                .filter_map(|(&domain, queue)| (!queue.is_empty()).then_some(domain))
                .collect();
            if ready.is_empty() {
                break;
            }
            let domain = ready[rng.index(ready.len())];
            let op = queues
                .get_mut(&domain)
                .expect("ready domain exists")
                .pop_front()
                .expect("ready queue is nonempty");
            execute_op(&mut simulator, domain, op, &mut deferred)
                .expect("generated legal operation must be admitted");
        }
        assert!(
            deferred.is_empty(),
            "every stage installs all prepared work"
        );
        simulator.finish_stage().expect("canonical effect barrier");

        for &domain in &stage.migrate {
            let old = simulator.token(domain).expect("active pre-migration token");
            let target = WorkerId(
                u16::try_from((usize::from(domain.0) + stage_index * 3 + 1) % usize::from(workers))
                    .expect("worker index fits u16"),
            );
            let handoff = simulator
                .begin_migration(old, target)
                .expect("legal quiescent migration begins");
            let new = simulator
                .commit_migration(handoff)
                .expect("exact handoff commits");
            assert_eq!(new.worker(), target);
            assert!(simulator.validate_token(old).is_err(), "old token is stale");
            simulator
                .validate_token(new)
                .expect("new generation owns authority");
        }

        stage_digests.push(
            simulator
                .semantic_digest()
                .expect("closed stage has a topology-independent digest"),
        );
    }

    stage_digests
}

#[test]
fn randomized_legal_schedules_are_semantically_identical_across_worker_counts() {
    let program = build_program();
    let baseline = run(&program, 1, 0xA076_1D64_78BD_642F);

    for workers in [1_u16, 2, 4, 8, 16] {
        for seed in [
            0xE703_7ED1_A0B4_28DB,
            0x8EBC_6AF0_9C88_C6E3,
            0x5899_65CC_7537_4CC3,
            0x1D8E_4E27_C47D_124F,
            0xEB44_ACCA_B455_D165,
            0xC6BC_2796_92B5_C323,
            0xD383_3E80_438F_1A61,
            0xDB4F_0B91_75AE_2165,
        ] {
            let candidate = run(&program, workers, seed ^ u64::from(workers));
            assert_eq!(
                candidate, baseline,
                "semantic history changed for workers={workers}, seed={seed:#x}"
            );
        }
    }
}

#[test]
fn illegal_authority_and_freshness_transitions_fail_without_state_change() {
    let mut simulator =
        OwnershipSimulator::new([(DomainId(0), WorkerId(0), 5), (DomainId(1), WorkerId(1), 8)])
            .expect("valid simulator");
    let domain0 = simulator.token(DomainId(0)).expect("domain 0 token");
    let domain1 = simulator.token(DomainId(1)).expect("domain 1 token");
    let work = simulator
        .prepare_deferred(domain0, DeferredId(900), 40, DeferredFreshness::ExactStamp)
        .expect("prepare exact work");

    assert_eq!(
        simulator.install_deferred(domain1, work),
        Err(OwnershipError::NotCurrentAuthority {
            domain: DomainId(0)
        })
    );
    assert_eq!(simulator.value(DomainId(0)), Ok(5));

    simulator.mutate(domain0, 1).expect("advance revision");
    assert_eq!(
        simulator.install_deferred(domain0, work),
        Err(OwnershipError::StaleDeferred {
            id: DeferredId(900)
        })
    );
    assert_eq!(simulator.value(DomainId(0)), Ok(6));

    simulator.finish_stage().expect("close stage");
    let handoff = simulator
        .begin_migration(domain0, WorkerId(7))
        .expect("begin migration");
    assert_eq!(
        simulator.begin_stage(),
        Err(OwnershipError::MigrationStillOpen {
            domain: DomainId(0)
        })
    );
    assert_eq!(
        simulator.validate_token(domain0),
        Err(OwnershipError::DomainMigrating {
            domain: DomainId(0)
        })
    );
    simulator
        .commit_migration(handoff)
        .expect("commit exact migration");
}
