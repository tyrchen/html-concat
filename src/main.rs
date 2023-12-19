use anyhow::Result;
use html_concat::aops::{AopsScraperBuilder, Challenge};
use std::fs;

#[tokio::main]
async fn main() -> Result<()> {
    let scraper = AopsScraperBuilder::default()
        .challenge(Challenge::Amc8)
        .years(2023..=2023)
        .problems(21..=25)
        .build()?;

    let mut ret = scraper.scrape().await?;
    let problems = ret.generate_problem()?;
    let solutions = ret.generate_solution()?;

    fs::write("aops.html", problems)?;
    fs::write("aops_solution.html", solutions)?;
    Ok(())
}
