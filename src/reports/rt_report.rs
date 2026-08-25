use anyhow::Result;
use ixa::{define_report, prelude::*, Context, ContextReportExt, HashMap, PluginContext};
use serde::{Deserialize, Serialize};

use crate::detection_manager::{PrimaryInfection, TransmissionChain};
use crate::shutdown::ShutdownEvent;
use crate::timekeeping::{ContextTimeKeeperExt, TimeKeeper};
use crate::{Person, PersonId};

#[derive(Serialize, Deserialize, Copy, Clone)]
struct OffspringReport {
    infector_id: PersonId,
    infection_time: f64,
    infection_date: TimeKeeper,
    offspring_count: usize,
}

#[derive(Serialize, Deserialize, Copy, Clone)]
struct GrowthRateReport {
    infection_date: TimeKeeper,
    infection_count: usize,
    total_offspring: usize,
    avg_onward_infections: f64,
}
define_report!(OffspringReport);
define_report!(GrowthRateReport);

struct GrowthRateData {
    data: HashMap<TimeKeeper, (usize, usize)>,
}
define_data_plugin!(
    GrowthRatePlugin,
    GrowthRateData,
    GrowthRateData {
        data: HashMap::default()
    }
);

trait OffspringReportExt: PluginContext + ContextReportExt + ContextTimeKeeperExt {
    fn collect_offspring_counts(&mut self, send_report: bool) {
        for person in self.get_entity_iterator::<Person>() {
            if let Some(data) = self.get_property::<Person, TransmissionChain>(person).0 {
                let infection_time = data.infection_time.into_inner();
                let offspring_count =
                    self.query_entity_count(with!(Person, PrimaryInfection(Some(person))));
                let report_entry = OffspringReport {
                    infector_id: person,
                    infection_time,
                    infection_date: self.get_date_at_time(infection_time),
                    offspring_count,
                };
                let map = self.get_data_mut(GrowthRatePlugin);
                map.data
                    .entry(report_entry.infection_date)
                    .and_modify(|(sum, count)| {
                        *sum += report_entry.offspring_count;
                        *count += 1;
                    })
                    .or_insert((report_entry.offspring_count, 1));
                if send_report {
                    self.send_report(report_entry);
                }
            }
        }
    }
    fn collect_rt_report(&mut self, send_report: bool) {
        let map = self.get_data(GrowthRatePlugin);
        for (date, (total_offspring, count)) in &map.data {
            let avg_onward_infections = if *count > 0 {
                *total_offspring as f64 / *count as f64
            } else {
                0.0
            };
            let report_entry = GrowthRateReport {
                infection_date: *date,
                infection_count: *count,
                total_offspring: *total_offspring,
                avg_onward_infections,
            };
            if send_report {
                self.send_report(report_entry);
            }
        }
    }
}
impl OffspringReportExt for Context {}

pub fn init(
    context: &mut Context,
    rt_name: Option<&str>,
    linelist_name: Option<&str>,
) -> Result<()> {
    let mut send_linelist = false;
    if let Some(linelist_filename) = linelist_name {
        context.add_report::<OffspringReport>(linelist_filename)?;
        send_linelist = true;
    }
    let mut send_rt = false;
    if let Some(rt_filename) = rt_name {
        context.add_report::<GrowthRateReport>(rt_filename)?;
        send_rt = true;
    }

    if send_linelist || send_rt {
        context.subscribe_to_event(move |context, _event: ShutdownEvent| {
            context.index_property::<Person, PrimaryInfection>();
            context.collect_offspring_counts(send_linelist);
            context.collect_rt_report(send_rt);
        });
    }
    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::detection_manager::TransmissionChainData;
    use crate::parameters::{ParameterValues, Parameters};
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn setup_context_with_transmission_chains() -> Context {
        let mut context = Context::new();
        context
            .set_global_property_value(
                Parameters,
                ParameterValues {
                    ..Default::default()
                },
            )
            .unwrap();

        // Generate linear transmission chain of 3 people
        let mut infector = None;
        for i in 0..3 {
            let person = if let Some(prev_infector) = infector {
                context
                    .add_entity(with!(
                        Person,
                        TransmissionChain(Some(TransmissionChainData::new_with_infector(
                            i as f64,
                            prev_infector,
                            Some(i - 1)
                        )))
                    ))
                    .unwrap()
            } else {
                context
                    .add_entity(with!(
                        Person,
                        TransmissionChain(Some(TransmissionChainData::new(0.0)))
                    ))
                    .unwrap()
            };
            infector = Some(person);
        }

        // Generate a new infector who infects 3 people
        let new_infector = context
            .add_entity(with!(
                Person,
                TransmissionChain(Some(TransmissionChainData::new(2.0)))
            ))
            .unwrap();
        for i in 0..3 {
            context
                .add_entity(with!(
                    Person,
                    TransmissionChain(Some(TransmissionChainData::new_with_infector(
                        3.0 + i as f64,
                        new_infector,
                        Some(i)
                    )))
                ))
                .unwrap();
        }

        context
    }

    fn assert_csv_records(file_path: PathBuf, expected: Vec<Vec<&'static str>>) {
        assert!(file_path.exists());
        let mut reader = csv::Reader::from_path(file_path).unwrap();
        let mut actual: Vec<Vec<String>> = reader
            .records()
            .map(|result| result.unwrap().iter().map(String::from).collect())
            .collect();
        let mut expected_strings: Vec<Vec<String>> = expected
            .into_iter()
            .map(|row| row.iter().map(|s| s.to_string()).collect())
            .collect();
        actual.sort();
        expected_strings.sort();
        assert_eq!(actual, expected_strings);
    }

    #[test]
    fn test_offspring_report() {
        let mut context = setup_context_with_transmission_chains();
        let temp_dir = tempdir().unwrap();
        let path = PathBuf::from(&temp_dir.path());
        context.report_options().directory(path.clone());

        context
            .add_report::<OffspringReport>("offspring_report")
            .unwrap();
        context.collect_offspring_counts(true);

        // Assert that the summary data by date is correct
        let map = &context.get_data(GrowthRatePlugin).data;
        assert_eq!(
            map.get(&(TimeKeeper::new(2025, 1, 1).unwrap())),
            Some(&(1, 1))
        );
        assert_eq!(
            map.get(&(TimeKeeper::new(2025, 1, 2).unwrap())),
            Some(&(1, 1))
        );
        assert_eq!(
            map.get(&(TimeKeeper::new(2025, 1, 3).unwrap())),
            Some(&(3, 2))
        );
        assert_eq!(
            map.get(&(TimeKeeper::new(2025, 1, 4).unwrap())),
            Some(&(0, 1))
        );
        assert_eq!(
            map.get(&(TimeKeeper::new(2025, 1, 5).unwrap())),
            Some(&(0, 1))
        );
        assert_eq!(
            map.get(&(TimeKeeper::new(2025, 1, 6).unwrap())),
            Some(&(0, 1))
        );

        std::mem::drop(context);
        assert_csv_records(
            path.join("offspring_report.csv"),
            vec![
                vec!["0", "0.0", "2025-01-01", "1"],
                vec!["1", "1.0", "2025-01-02", "1"],
                vec!["2", "2.0", "2025-01-03", "0"],
                vec!["3", "2.0", "2025-01-03", "3"],
                vec!["4", "3.0", "2025-01-04", "0"],
                vec!["5", "4.0", "2025-01-05", "0"],
                vec!["6", "5.0", "2025-01-06", "0"],
            ],
        );
    }

    #[test]
    fn test_rt_report() {
        let mut context = setup_context_with_transmission_chains();
        let temp_dir = tempdir().unwrap();
        let path = PathBuf::from(&temp_dir.path());
        context.report_options().directory(path.clone());

        context.add_report::<GrowthRateReport>("rt_report").unwrap();
        context.collect_offspring_counts(false);
        context.collect_rt_report(true);

        std::mem::drop(context);
        assert_csv_records(
            path.join("rt_report.csv"),
            vec![
                vec!["2025-01-01", "1", "1", "1.0"],
                vec!["2025-01-02", "1", "1", "1.0"],
                vec!["2025-01-03", "2", "3", "1.5"],
                vec!["2025-01-04", "1", "0", "0.0"],
                vec!["2025-01-05", "1", "0", "0.0"],
                vec!["2025-01-06", "1", "0", "0.0"],
            ],
        );
    }
}
