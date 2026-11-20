#!/usr/bin/env Rscript
# Statistical analysis of Home Assistant sensor history aggregated by the
# Rust binary. Reads the hourly CSV and writes a summary plus a PNG plot.
#
# Usage: Rscript analyze.R <input.csv> <output_dir>

args <- commandArgs(trailingOnly = TRUE)
if (length(args) < 2) {
  stop("usage: Rscript analyze.R <input.csv> <output_dir>", call. = FALSE)
}
input_csv <- args[1]
out_dir <- args[2]

if (!file.exists(input_csv)) {
  stop(sprintf("input not found: %s", input_csv), call. = FALSE)
}
dir.create(out_dir, showWarnings = FALSE, recursive = TRUE)

df <- read.csv(input_csv, stringsAsFactors = FALSE)
if (nrow(df) == 0) {
  stop("input CSV has no rows", call. = FALSE)
}

df$time <- as.POSIXct(df$hour_start_utc, origin = "1970-01-01", tz = "UTC")
df$entity <- sub("^sensor\\.rust_", "", df$entity_id)

cat("=== Hourly buckets read ===\n")
print(df[, c("entity", "time", "n", "min", "max", "mean")], row.names = FALSE)

cat("\n=== Per-entity summary (mean of hourly means) ===\n")
summary_tbl <- do.call(rbind, lapply(split(df, df$entity), function(g) {
  data.frame(
    entity = g$entity[1],
    samples = sum(g$n),
    mean = mean(g$mean),
    sd = if (sum(g$n) > 1) sd(g$mean) else NA_real_,
    min = min(g$min),
    max = max(g$max),
    cv_pct = if (mean(g$mean) != 0) sd(g$mean) / mean(g$mean) * 100 else NA_real_
  )
}))
print(summary_tbl, row.names = FALSE, digits = 4)

write.csv(summary_tbl, file.path(out_dir, "summary.csv"), row.names = FALSE)

png(file.path(out_dir, "sensor_history.png"), width = 1200, height = 800, res = 120)
par(mfrow = c(nlevels(factor(df$entity)), 1), mar = c(4, 4, 2, 1))
cols <- c("steelblue", "darkorange", "forestgreen")
for (i in seq_along(levels(factor(df$entity)))) {
  e <- levels(factor(df$entity))[i]
  g <- df[df$entity == e, ]
  g <- g[order(g$time), ]
  plot(g$time, g$mean,
    type = "b", pch = 19, col = cols[(i - 1) %% length(cols) + 1],
    xlab = "Time (UTC)", ylab = "Value", main = e
  )
  arrows(g$time, g$min, g$time, g$max, angle = 90, code = 3, length = 0.03, col = "grey40")
}
dev.off()

cat(sprintf("\nWrote %s and %s\n", file.path(out_dir, "summary.csv"), file.path(out_dir, "sensor_history.png")))
