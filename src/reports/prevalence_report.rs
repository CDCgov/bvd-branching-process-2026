use crate::Person;
use crate::{
    clinical_manager::{CaseStatus, CaseStatusValue, HealthSetting, TreatmentLocation},
    detection_manager::{DetectionStatus, Generation},
    disease_manager::{Alive, InfectionStatus},
    timekeeping::{ContextTimeKeeperExt, TimeKeeper},
};
use anyhow::Result;
use ixa::{
    define_data_plugin, define_report, impl_derived_property, prelude::*, Context,
    ContextReportExt, ExecutionPhase, HashMap,
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
struct PersonPropertyReport {
    t: f64,
    date: TimeKeeper,
    infection_status: InfectionStatus,
    alive: bool,
    generation: Option<usize>,
    case_status: Option<CaseStatusValue>,
    detection_status: DetectionStatus,
    treatment_location: Option<HealthSetting>,
    count: usize,
}

define_report!(PersonPropertyReport);

#[derive(Eq, Hash, PartialEq, Serialize, Deserialize, Copy, Clone, Debug)]
pub struct PersonReportProperties {
    infection_status: InfectionStatus,
    alive: bool,
    generation: Option<usize>,
    case_status: Option<CaseStatusValue>,
    detection_status: DetectionStatus,
    treatment_location: Option<HealthSetting>,
}

impl_derived_property!(
    PersonReportProperties,
    Person,
    [
        InfectionStatus,
        Alive,
        Generation,
        CaseStatus,
        DetectionStatus,
        TreatmentLocation
    ],
    [],
    |infection_status, alive, generation, case_status, detection_status, treatment_location| {
        PersonReportProperties {
            infection_status,
            alive: alive.0,
            generation: generation.0,
            case_status: case_status.0,
            detection_status,
            treatment_location: treatment_location.0,
        }
    }
);

struct PropertyReportDataContainer {
    report_map_container: HashMap<PersonReportProperties, usize>,
}

define_data_plugin!(
    PropertyReportDataPlugin,
    PropertyReportDataContainer,
    PropertyReportDataContainer {
        report_map_container: HashMap::default(),
    }
);

type ReportEvent = PropertyChangeEvent<Person, PersonReportProperties>;

fn update_property_change_counts(context: &mut Context, event: ReportEvent) {
    let report_container_mut = context.get_data_mut(PropertyReportDataPlugin);

    report_container_mut
        .report_map_container
        .entry(event.current)
        .and_modify(|n| *n += 1)
        .or_insert(1);

    report_container_mut
        .report_map_container
        .entry(event.previous)
        .and_modify(|n| *n -= 1)
        .or_insert(0);
}

fn add_property_change_counts(context: &mut Context, event: EntityCreatedEvent<Person>) {
    let values = context.get_property::<Person, PersonReportProperties>(event.entity_id);
    let report_container_mut = context.get_data_mut(PropertyReportDataPlugin);
    report_container_mut
        .report_map_container
        .entry(values)
        .and_modify(|n| *n += 1)
        .or_insert(1);
}

fn send_property_counts(context: &mut Context) {
    let report_container = context.get_data(PropertyReportDataPlugin);

    for (values, count_property) in &report_container.report_map_container {
        context.send_report(PersonPropertyReport {
            t: context.get_current_time(),
            date: context.get_current_date(),
            infection_status: values.infection_status,
            alive: values.alive,
            generation: values.generation,
            case_status: values.case_status,
            detection_status: values.detection_status,
            treatment_location: values.treatment_location,
            count: *count_property,
        });
    }
}

/// Count initial number of people per property status and subscribe to cahnges
/// # Errors
///
/// Will return `IxaError` if the report cannot be added
///
/// # Panics
///
/// Will panic if symptom value string is not listed in enum
pub fn init(context: &mut Context, file_name: &str, period: f64) -> Result<()> {
    context.add_report::<PersonPropertyReport>(file_name)?;

    let mut map_counts = HashMap::default();

    for person in context.query_result_iterator(Person) {
        let value = context.get_property::<Person, PersonReportProperties>(person);
        map_counts
            .entry(value)
            .and_modify(|count| *count += 1)
            .or_insert(1);
    }

    let report_container = context.get_data_mut(PropertyReportDataPlugin);
    report_container.report_map_container = map_counts;

    context.subscribe_to_event(|context, event: ReportEvent| {
        update_property_change_counts(context, event);
    });
    context.subscribe_to_event(|context, event: EntityCreatedEvent<Person>| {
        add_property_change_counts(context, event);
    });

    context.add_periodic_plan_with_phase(
        period,
        move |context: &mut Context| {
            send_property_counts(context);
        },
        ExecutionPhase::Last,
    );
    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{
        clinical_manager::{CaseStatus, CaseStatusValue, HealthSetting, TreatmentLocation},
        detection_manager::{TransmissionChain, TransmissionChainData},
        disease_manager::{Alive, InfectionStatus},
        infection_initialization::{InfectionInitialization, NovelInfectionSource},
        parameters::{ContextParametersExt, ParameterValues, Parameters},
        reports::{ReportParams, Reports},
        shutdown::{plan_shutdown, EndRunConditions},
        timekeeping::TimeKeeper,
    };
    use ixa::{Context, ContextGlobalPropertiesExt, ContextRandomExt, ContextReportExt};
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn setup_context_with_report(prevalence_report: ReportParams) -> Context {
        let mut context = Context::new();
        context
            .set_global_property_value(
                Parameters,
                ParameterValues {
                    initialization: InfectionInitialization {
                        start_date: TimeKeeper::new(2026, 1, 15).unwrap(),
                        initial_cases: NovelInfectionSource::None,
                    },
                    end_run_conditions: EndRunConditions::from_max_time(3.0),
                    reports: Reports {
                        prevalence_report,
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
    fn test_generate_prevalence_report() {
        let mut context = setup_context_with_report(ReportParams {
            write: true,
            filename: Some("output.csv".to_string()),
            period: Some(2.0),
            trigger: None,
        });

        let temp_dir = tempdir().unwrap();
        let path = PathBuf::from(&temp_dir.path());
        let config = context.report_options();
        config.directory(path.clone());

        let confirmed = context
            .add_entity(with!(
                Person,
                CaseStatus(Some(CaseStatusValue::Confirmed)),
                TreatmentLocation(Some(HealthSetting::EbolaTreatmentUnit)),
                InfectionStatus::Symptomatic,
                TransmissionChain(Some(TransmissionChainData::new(0.0))),
            ))
            .unwrap();
        let suspected = context
            .add_entity(with!(
                Person,
                CaseStatus(Some(CaseStatusValue::Suspected)),
                TransmissionChain(Some(TransmissionChainData::new_with_infector(
                    0.0,
                    confirmed,
                    Some(0)
                ))),
            ))
            .unwrap();

        let confirmation_time = 1.0;

        crate::reports::init(&mut context).unwrap();
        let end_run_conditions = context.get_params().end_run_conditions;
        plan_shutdown(&mut context, end_run_conditions).unwrap();

        // Change person properties and add a new case
        context.add_plan(confirmation_time, move |context| {
            context.set_property(suspected, CaseStatus(Some(CaseStatusValue::Confirmed)));
            context.set_property(confirmed, Alive(false));
            context
                .add_entity(with!(
                    Person,
                    TransmissionChain(Some(TransmissionChainData::new_with_infector(
                        0.0,
                        suspected,
                        Some(1)
                    ))),
                ))
                .unwrap();
        });
        context.execute();

        let prevalence_report = context.get_params().reports.prevalence_report.clone();
        let file_path = if let Some(name) = prevalence_report.filename {
            path.join(name)
        } else {
            panic!("No report name specified");
        };

        assert!(file_path.exists());
        std::mem::drop(context);

        assert!(file_path.exists());
        let mut reader = csv::Reader::from_path(file_path).unwrap();

        let mut actual: Vec<Vec<String>> = reader
            .records()
            .map(|result| result.unwrap().iter().map(String::from).collect())
            .collect();
        let mut expected = vec![
            //  [t, date, infection_status, alive, generation, case_status, detection_status, treatment_location]
            vec![
                "0.0",
                "2026-01-15",
                "Symptomatic",
                "true",
                "0",
                "Confirmed",
                "Undetected",
                "EbolaTreatmentUnit",
                "1",
            ],
            vec![
                "0.0",
                "2026-01-15",
                "Presymptomatic",
                "true",
                "1",
                "Suspected",
                "Undetected",
                "",
                "1",
            ],
            vec![
                "2.0",
                "2026-01-17",
                "Symptomatic",
                "false",
                "0",
                "Confirmed",
                "Undetected",
                "EbolaTreatmentUnit",
                "1",
            ],
            vec![
                "2.0",
                "2026-01-17",
                "Symptomatic",
                "true",
                "0",
                "Confirmed",
                "Undetected",
                "EbolaTreatmentUnit",
                "0",
            ],
            vec![
                "2.0",
                "2026-01-17",
                "Presymptomatic",
                "true",
                "1",
                "Confirmed",
                "Undetected",
                "",
                "1",
            ],
            // Only an initialized combination can have a zero count
            vec![
                "2.0",
                "2026-01-17",
                "Presymptomatic",
                "true",
                "1",
                "Suspected",
                "Undetected",
                "",
                "0",
            ],
            vec![
                "2.0",
                "2026-01-17",
                "Presymptomatic",
                "true",
                "2",
                "",
                "Undetected",
                "",
                "1",
            ],
        ];

        actual.sort();
        expected.sort();

        assert_eq!(actual, expected, "CSV file should contain the correct data");
    }
}
