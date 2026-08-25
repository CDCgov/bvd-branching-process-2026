use crate::clinical_manager::{CaseStatus, CaseStatusValue};
use crate::timekeeping::{ContextTimeKeeperExt, TimeKeeper};
use crate::transmission_manager::ContextTransmissionExt;
use crate::{Person, PersonId};
use anyhow::Result;
use ixa::{
    define_data_plugin, define_report, prelude::*, Context, ContextReportExt, ExecutionPhase,
    HashMap,
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct CaseStatusIncidenceReport {
    t_upper: f64,
    date: TimeKeeper,
    case_status: Option<CaseStatusValue>,
    count: usize,
    with_confirmed_index: usize,
}

define_report!(CaseStatusIncidenceReport);

struct StatusIncidenceData {
    incident: usize,
    with_confirmed_index: usize,
}

struct CaseStatusIncidenceContainer {
    // Increment on novel people for each time step
    people: HashMap<PersonId, bool>,
    case_status_map: HashMap<CaseStatus, StatusIncidenceData>,
}

define_data_plugin!(
    CaseStatusDataPlugin,
    CaseStatusIncidenceContainer,
    CaseStatusIncidenceContainer {
        people: HashMap::default(),
        case_status_map: HashMap::default(),
    }
);

fn update_incidence(context: &mut Context, event: PropertyChangeEvent<Person, CaseStatus>) {
    // Determine if the person was infected by a confirmed case
    // If an index case becomes confirmed afterwards, do not include the secondary as "from confirmed"
    // This is to align with the indicator definition of "percent of newly confirmed cases that arise from known chains of transmission"
    let from_confirmed = context.is_from_confirmed(event.entity_id);

    let report_container = context.get_data_mut(CaseStatusDataPlugin);

    // Only track incidence in one category per report period for each person
    if let Some(previous) = report_container
        .people
        .insert(event.entity_id, from_confirmed)
    {
        report_container
            .case_status_map
            .entry(event.previous)
            .and_modify(|v| {
                v.incident -= 1;
                if previous {
                    v.with_confirmed_index -= 1;
                }
            });
    }

    report_container
        .case_status_map
        .entry(event.current)
        .and_modify(|v| {
            v.incident += 1;
            if from_confirmed {
                v.with_confirmed_index += 1;
            }
        })
        .or_insert(StatusIncidenceData {
            incident: 1,
            with_confirmed_index: if from_confirmed { 1 } else { 0 },
        });
}

fn reset_incidence_map(context: &mut Context) {
    let report_container = context.get_data_mut(CaseStatusDataPlugin);
    report_container.case_status_map.values_mut().for_each(|v| {
        v.incident = 0;
        v.with_confirmed_index = 0;
    });
    report_container.people.clear();
}

fn send_incidence_counts(context: &mut Context) {
    let report_container = context.get_data(CaseStatusDataPlugin);
    let t_upper = context.get_current_time();

    // Case status
    for (status, data) in &report_container.case_status_map {
        context.send_report(CaseStatusIncidenceReport {
            t_upper,
            date: context.get_current_date(),
            case_status: status.0,
            count: data.incident,
            with_confirmed_index: data.with_confirmed_index,
        });
    }
    reset_incidence_map(context);
}

pub fn init(context: &mut Context, file_name: &str, period: f64) -> Result<()> {
    context.add_report::<CaseStatusIncidenceReport>(file_name)?;

    context.subscribe_to_event(|context, event: PropertyChangeEvent<Person, CaseStatus>| {
        update_incidence(context, event);
    });

    context.add_periodic_plan_with_phase(
        period,
        move |context: &mut Context| {
            send_incidence_counts(context);
        },
        ExecutionPhase::Last,
    );

    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{
        detection_manager::{TransmissionChain, TransmissionChainData},
        parameters::{ContextParametersExt, ParameterValues, Parameters},
        reports::{ReportParams, Reports},
        shutdown::{plan_shutdown, EndRunConditions},
    };

    use std::path::PathBuf;
    use tempfile::tempdir;

    fn setup_context_with_report(confirmation_incidence_report: ReportParams) -> Context {
        let mut context = Context::new();
        let end_run_conditions = EndRunConditions::from_max_time(5.0);
        plan_shutdown(&mut context, end_run_conditions).unwrap();
        context
            .set_global_property_value(
                Parameters,
                ParameterValues {
                    end_run_conditions,
                    reports: Reports {
                        confirmation_incidence_report,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
            .unwrap();
        context.init_random(context.get_params().seed);
        context
    }

    #[test]
    fn test_confirmation_report() {
        let mut context = setup_context_with_report(ReportParams {
            write: true,
            filename: Some("output.csv".to_string()),
            period: Some(1.0),
            trigger: None,
        });

        let temp_dir = tempdir().unwrap();
        let path = PathBuf::from(&temp_dir.path());
        let config = context.report_options();
        config.directory(path.clone());

        // Set up two index cases, one confirmed
        let confirmed_index = context
            .add_entity(with!(Person, CaseStatus(Some(CaseStatusValue::Confirmed))))
            .unwrap();
        let undetected_index = context.add_entity(Person).unwrap();

        // infect three individuals from each index at t=1.0
        for i in 0..3 {
            let p1 = context
                .add_entity(with!(
                    Person,
                    TransmissionChain(Some(TransmissionChainData::new_with_infector(
                        0.0,
                        confirmed_index,
                        Some(0)
                    )))
                ))
                .unwrap();
            context.add_plan(0.1 + i as f64, move |context| {
                context.set_property::<Person, CaseStatus>(
                    p1,
                    CaseStatus(Some(CaseStatusValue::Suspected)),
                );
            });
            context.add_plan(0.3 + i as f64, move |context| {
                context.set_property::<Person, CaseStatus>(
                    p1,
                    CaseStatus(Some(CaseStatusValue::Confirmed)),
                );
            });
            let p2 = context
                .add_entity(with!(
                    Person,
                    TransmissionChain(Some(TransmissionChainData::new_with_infector(
                        1.0,
                        undetected_index,
                        Some(0)
                    )))
                ))
                .unwrap();

            // Move between suspected and confirmed within the timestep
            context.add_plan(0.1 + i as f64, move |context| {
                context.set_property::<Person, CaseStatus>(
                    p2,
                    CaseStatus(Some(CaseStatusValue::Suspected)),
                );
            });
            context.add_plan(0.3 + i as f64, move |context| {
                context.set_property::<Person, CaseStatus>(
                    p2,
                    CaseStatus(Some(CaseStatusValue::Confirmed)),
                );
            });
        }

        // Change the originally undetected inidividual during the suspected->confirmed transition of i=1
        context.add_plan(1.2, move |context| {
            context.set_property::<Person, CaseStatus>(
                undetected_index,
                CaseStatus(Some(CaseStatusValue::Confirmed)),
            );
        });

        crate::reports::init(&mut context).unwrap();
        context.execute();

        let confirmation_incidence_report = context
            .get_params()
            .reports
            .confirmation_incidence_report
            .clone();
        let file_path = if let Some(name) = confirmation_incidence_report.filename {
            path.join(name)
        } else {
            panic!("No report name specified");
        };

        assert!(file_path.exists());
        std::mem::drop(context);
        let mut reader = csv::Reader::from_path(file_path).unwrap();

        let mut actual: Vec<Vec<String>> = reader
            .records()
            .map(|result| result.unwrap().iter().map(String::from).collect())
            .collect();
        let mut expected = vec![
            vec!["1.0", "2025-01-02", "Suspected", "0", "0"],
            vec!["2.0", "2025-01-03", "Suspected", "0", "0"],
            vec!["3.0", "2025-01-04", "Suspected", "0", "0"],
            vec!["4.0", "2025-01-05", "Suspected", "0", "0"],
            vec!["5.0", "2025-01-06", "Suspected", "0", "0"],
            vec!["1.0", "2025-01-02", "Confirmed", "2", "1"],
            vec!["2.0", "2025-01-03", "Confirmed", "3", "2"],
            vec!["3.0", "2025-01-04", "Confirmed", "2", "2"],
            vec!["4.0", "2025-01-05", "Confirmed", "0", "0"],
            vec!["5.0", "2025-01-06", "Confirmed", "0", "0"],
        ];
        actual.sort();
        expected.sort();
        assert_eq!(actual, expected);
    }
}
