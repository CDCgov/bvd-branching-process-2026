#### DRC Ebola 2026: Tech Brief

## Script originally prepared by Inga Holmdahl

## Two input files are needed for this script
## 1. target_data.csv
## 2. all_simulations.csv

## This script uses Nunito Sans font.
## https://fonts.google.com/specimen/Nunito+Sans
## Colors are from CDC color palette.

library(dotenv)
library(lubridate)
library(readr)
library(dplyr)
library(tidyr)
library(ggplot2)
library(ggthemes)
library(ragg)

# load environment variable for output directory
dotenv::load_dot_env(file = ".env")

products_path <- file.path(
  Sys.getenv("OUTPUT_DIR"),
  "experiments",
  "bvd",
  "detection",
  "detection_1.00",
  "products"
)

projection_path <- file.path(
  Sys.getenv("OUTPUT_DIR"),
  "experiments",
  "bvd",
  "detection",
  "detection_1.00",
  "projection"
)

targets_path <- file.path(products_path, "target_data.csv")
sims_path <- file.path(projection_path, "all_simulations.csv")

overall_detect_plot_png_path <- file.path(
  products_path,
  "overall_detection_panels.png"
)
overall_detect_plot_svg_path <- file.path(
  products_path,
  "overall_detection_panels.svg"
)
example_traj_plot_png_path <- file.path(
  products_path,
  "individual_trajectories_examples.png"
)
example_traj_plot_svg_path <- file.path(
  products_path,
  "individual_trajectories_examples.svg"
)

end_data_point <- lubridate::ymd("2026-09-06")

# read in target data
targets <- readr::read_csv(targets_path) |>
  mutate(week_end = epiweek_startdate + lubridate::days(6)) |>
  select(week_end, cumul_cases = cumulative_confirmation_incidence)

targets_ref <- targets |>
  pivot_wider(
    names_from = week_end,
    values_from = cumul_cases,
    names_prefix = "week_"
  )

# read in model trajectories
sims <- readr::read_csv(sims_path) |>
  select(-num_days_in_epiweek) |>
  mutate(week_end = epiweek_startdate + lubridate::days(6)) |>
  rename(
    inc = confirmation_incidence,
    cumul_cases = cumulative_confirmation_incidence
  )

catlabels <- c("20-30%", "30-40%", "40-50%", "50-60%")

res <- sims |>
  mutate(
    early_target = targets_ref$`week_2026-06-21`,
    overall_target = targets_ref$`week_2026-09-06`
  ) |>
  group_by(particle_id) |>
  mutate(
    cumul_cases_beforejune1 = replace_na(
      cumul_cases[week_end == as.Date("2026-05-31")][1],
      0
    ),
    cumul_cases_june21 = cumul_cases[week_end == as.Date("2026-06-21")][1],
    cumul_cases_end = cumul_cases[week_end == end_data_point][1],
    cumul_cases_since_june1 = cumul_cases - cumul_cases_beforejune1,
    new_cases_june21 = cumul_cases_june21 - cumul_cases_beforejune1,
    new_cases_end = cumul_cases_end - cumul_cases_beforejune1,
    early_detection_rate = early_target / new_cases_june21,
    overall_detection_rate = overall_target / new_cases_end,
    early_cat = cut(
      early_detection_rate,
      breaks = c(0.2, 0.3, 0.4, 0.5, 0.6),
      right = FALSE,
      labels = catlabels
    ),
    overall_cat = cut(
      overall_detection_rate,
      breaks = c(0.2, 0.3, 0.4, 0.5, 0.6),
      right = FALSE,
      labels = catlabels
    )
  ) |>
  ungroup()

stopifnot(!is.na(res))

## calculate scaled target values for annotations
plot_targets <- targets |>
  mutate(
    cutoff20 = cumul_cases / 0.2,
    cutoff30 = cumul_cases / 0.3,
    cutoff40 = cumul_cases / 0.4,
    cutoff50 = cumul_cases / 0.5,
    cutoff60 = cumul_cases / 0.6
  ) |>
  select(-cumul_cases) |>
  pivot_longer(cols = cutoff20:cutoff60, names_to = "level")

# overall detection rate - facets
percents <- res[!duplicated(res$particle_id), ] |>
  count(overall_cat) |>
  mutate(proportion_of_trajectories = n / sum(n))

stopifnot(sum(percents$proportion_of_trajectories) == 1)

percents <- percents |>
  mutate(percent_of_trajectories = round(proportion_of_trajectories * 100))

facet_labels <- as_labeller(c(
  "20-30%" = paste0(
    "20\u201330% of cases detected\n(",
    percents$percent_of_trajectories[percents$overall_cat == "20-30%"],
    "% of trajectories)"
  ),
  "30-40%" = paste0(
    "30\u201340% of cases detected\n(",
    percents$percent_of_trajectories[percents$overall_cat == "30-40%"],
    "% of trajectories)"
  ),
  "40-50%" = paste0(
    "40\u201350% of cases detected\n(",
    percents$percent_of_trajectories[percents$overall_cat == "40-50%"],
    "% of trajectories)"
  ),
  "50-60%" = paste0(
    "50\u201360% of cases detected\n(",
    percents$percent_of_trajectories[percents$overall_cat == "50-60%"],
    "% of trajectories)"
  )
))

# get median line
res_median <- res |>
  filter(week_end <= end_data_point) |>
  group_by(overall_cat, week_end) |>
  summarise(median = median(cumul_cases_since_june1))

overall_detect_plot <- res |>
  filter(week_end <= end_data_point) |>
  ggplot() +
  geom_line(
    aes(x = week_end, y = cumul_cases_since_june1, group = particle_id),
    color = "#3382CF",
    alpha = 0.025
  ) +
  geom_line(
    data = res_median,
    aes(x = week_end, y = median),
    color = "#032659",
    alpha = 1,
    linewidth = 0.9
  ) +
  coord_cartesian(ylim = c(0, 31000)) +
  geom_point(data = targets, aes(x = week_end, y = cumul_cases)) +
  scale_x_date(date_labels = "%b %e", date_breaks = "1 month") +
  facet_wrap(~overall_cat, nrow = 1, labeller = facet_labels) +
  labs(
    x = "Date",
    y = "Cumulative cases (detected and undetected) since June 1"
  ) +
  guides(colour = guide_legend(override.aes = list(alpha = 1, linewidth = 2))) +
  ggthemes::theme_clean() +
  theme(plot.background = element_rect(color = NA)) +
  theme(text = element_text(family = "Nunito Sans")) +
  theme(axis.text.x = element_text(size = 8))

ggsave(
  overall_detect_plot_png_path,
  plot = overall_detect_plot,
  units = "in",
  width = 8,
  height = 4.5,
  dpi = 300,
  device = ragg::agg_png
)
ggsave(
  overall_detect_plot_svg_path,
  plot = overall_detect_plot,
  units = "in",
  width = 8,
  height = 4.5,
  device = svg
)

# color by groups: TREND in detection - Ribbon
trend_data <- res |>
  mutate(
    trend = case_when(
      (early_cat == "20-30%" &
        overall_cat == "50-60%") ~ "Strongly improving detection",
      (early_cat == "50-60%" &
        overall_cat == "20-30%") ~ "Strongly worsening detection",
      (early_cat == "30-40%" &
        overall_cat == "40-50%") ~ "Slightly improving detection",
      (early_cat == "40-50%" &
        overall_cat == "30-40%") ~ "Slightly worsening detection",
      TRUE ~ NA
    ),
    trend = factor(
      trend,
      levels = c(
        "Strongly worsening detection",
        "Slightly worsening detection",
        "Slightly improving detection",
        "Strongly improving detection"
      )
    )
  )


## highlight some individual trajectories that show our point
## select example strongly worsening and strongly improving particles

improving_examples <- trend_data[
  trend_data$week_end == end_data_point &
    trend_data$trend == "Strongly improving detection" &
    !is.na(trend_data$trend),
]

improving_ex_id <- improving_examples$particle_id[
  which.min(abs(
    improving_examples$new_cases_end - median(improving_examples$new_cases_end)
  ))
]

worsening_examples <- trend_data[
  trend_data$week_end == end_data_point &
    trend_data$trend == "Strongly worsening detection" &
    !is.na(trend_data$trend),
]

worsening_ex_id <- worsening_examples$particle_id[
  which.min(abs(
    worsening_examples$new_cases_end - median(worsening_examples$new_cases_end)
  ))
]

trend_colors <- c(
  "Strongly worsening detection" = "#712061",
  "Strongly improving detection" = "#FFB24D"
)

example_traj_plot <- trend_data |>
  filter(
    !is.na(trend),
    week_end <= end_data_point,
    particle_id %in% c(improving_ex_id, worsening_ex_id)
  ) |>
  ggplot() +
  geom_line(
    data = filter(res, week_end <= end_data_point),
    aes(x = week_end, y = cumul_cases_since_june1, group = particle_id),
    alpha = 0.01
  ) +
  geom_segment(
    data = targets,
    aes(x = week_end, y = cumul_cases / 0.6, yend = cumul_cases / 0.2),
    color = "black",
    linewidth = 0.6,
    alpha = 0.6
  ) +
  geom_segment(
    data = plot_targets,
    aes(
      x = week_end - lubridate::days(1),
      xend = week_end + lubridate::days(1),
      y = value,
      group = level
    ),
    color = "black",
    linewidth = 0.6,
    alpha = 0.6
  ) +
  geom_line(
    aes(x = week_end, y = cumul_cases_since_june1, color = trend),
    alpha = 0.8,
    linewidth = 1
  ) +
  coord_cartesian(
    ylim = c(0, 32000),
    xlim = c(lubridate::ymd("2026-06-01"), end_data_point),
    clip = "off"
  ) +
  annotate(
    "text",
    x = targets$week_end[2] + 1,
    y = targets$cumul_cases[2],
    label = "Cumulative cases reported June 1\u2013June 21",
    hjust = 0,
    vjust = 1,
    size = 3,
    family = "Nunito Sans"
  ) +
  annotate(
    "text",
    x = targets$week_end[1] + 1,
    y = targets$cumul_cases[1],
    label = "Cumulative cases reported\nJune 1\u2013September 6",
    hjust = 0,
    vjust = 1,
    size = 3,
    lineheight = 0.9,
    family = "Nunito Sans"
  ) +
  annotate(
    "text",
    x = end_data_point + lubridate::days(1),
    y = plot_targets$value[plot_targets$week_end == end_data_point],
    label = c("Detection\n20%", "30%", "40%", "50%", "60%"),
    hjust = 0,
    vjust = c(0.15, 0.5, 0.5, 0.5, 0.5),
    size = 3,
    family = "Nunito Sans"
  ) +
  geom_point(data = targets, aes(x = week_end, y = cumul_cases)) +
  scale_color_manual(values = trend_colors) +
  scale_x_date(date_labels = "%B %e", date_breaks = "1 month") +
  labs(
    x = "Date",
    y = "Cumulative cases (detected and undetected) since June 1"
  ) +
  ggthemes::theme_clean() +
  theme(text = element_text(family = "Nunito Sans")) +
  theme(legend.position = "none") +
  theme(plot.margin = margin(t = 5.5, r = 120, b = 5.5, l = 5.5)) +
  theme(plot.background = element_rect(color = NA))

arrow_ends <- example_traj_plot$data |>
  group_by(trend) |>
  filter(week_end == max(week_end)) |>
  ungroup()

example_traj_plot <- example_traj_plot +
  annotate(
    "segment",
    x = arrow_ends$week_end + lubridate::days(10),
    xend = arrow_ends$week_end + lubridate::days(1),
    y = arrow_ends$cumul_cases_since_june1,
    yend = arrow_ends$cumul_cases_since_june1,
    color = trend_colors[as.character(arrow_ends$trend)],
    linewidth = 0.8,
    arrow = arrow(length = unit(0.07, "inches"), type = "closed")
  ) +
  annotate(
    "text",
    x = arrow_ends$week_end + lubridate::days(10),
    y = arrow_ends$cumul_cases_since_june1,
    label = c(
      "If detection has gotten\nbetter over time, the\noutbreak may be\ngrowing more slowly",
      "If detection has gotten\nworse over time, the\noutbreak may be\ngrowing rapidly"
    ),
    hjust = 0,
    vjust = 0.5,
    size = 3,
    lineheight = 0.9,
    family = "Nunito Sans"
  )

ggsave(
  example_traj_plot_png_path,
  plot = example_traj_plot,
  units = "in",
  width = 8,
  height = 4.5,
  dpi = 300,
  device = ragg::agg_png
)
ggsave(
  example_traj_plot_svg_path,
  plot = example_traj_plot,
  units = "in",
  width = 8,
  height = 4.5,
  dpi = 300,
  device = svg
)
