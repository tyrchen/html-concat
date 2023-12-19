use anyhow::{Context, Result};
use askama::Template;
use derive_builder::Builder;
use html5ever::tree_builder::TreeSink;
use scraper::{ElementRef, Html, Selector};
use serde::{Deserialize, Serialize};
use std::ops::RangeInclusive;
use strum::{Display, EnumString};

#[derive(Debug, Builder, Serialize, Deserialize)]
pub struct AopsScraper {
    #[builder(setter(into))]
    years: Vec<RangeInclusive<u32>>,
    problems: RangeInclusive<u32>,
    challenge: Challenge,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AopsProblem {
    year: u32,
    number: u32,
    problem: String,
    solution: String,
}

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, EnumString, Display)]
pub enum Challenge {
    #[default]
    #[strum(serialize = "AMC_8")]
    #[serde(rename = "AMC_8")]
    Amc8,
    #[strum(serialize = "AMC_10A")]
    #[serde(rename = "AMC_10A")]
    Amc10a,
    #[strum(serialize = "AMC_10B")]
    #[serde(rename = "AMC_10B")]
    Amc10b,
}

#[derive(Debug, Default, Template)]
#[template(path = "aops.html.j2")]
pub struct AopsScrapeResult {
    pub styles: Vec<String>,
    pub challenge: Challenge,
    pub is_solution: bool,
    pub contents: Vec<AopsContent>,
}

#[derive(Debug)]
pub struct AopsContent {
    pub year: u32,
    pub problems: Vec<AopsProblem>,
}

impl AopsScraper {
    pub async fn scrape(self) -> Result<AopsScrapeResult> {
        let mut years = vec![];
        for r in self.years {
            years.extend(r);
        }
        Self::scrape_all(years, self.problems, self.challenge).await
    }

    async fn scrape_all(
        years: Vec<u32>,
        problems: RangeInclusive<u32>,
        challenge: Challenge,
    ) -> Result<AopsScrapeResult> {
        let mut contents = vec![];
        let mut handles = vec![];
        let mut styles = vec![];

        for year in years {
            let problems = problems.clone();
            let handle =
                tokio::spawn(async move { Self::scrape_problems(year, problems, challenge).await });

            handles.push(handle);
        }

        for handle in handles {
            let (content, style_data) = handle.await??;
            if styles.is_empty() {
                styles = style_data;
            }
            contents.push(content);
        }

        Ok(AopsScrapeResult {
            styles,
            challenge,
            is_solution: false,
            contents,
        })
    }

    async fn scrape_problems(
        year: u32,
        problems: RangeInclusive<u32>,
        challenge: Challenge,
    ) -> Result<(AopsContent, Vec<String>)> {
        let mut styles = vec![];
        let mut content = AopsContent::new(year);
        let mut handles = vec![];
        for problem in problems {
            let url = get_url(year, problem, challenge);
            let handle = tokio::spawn(async move {
                let html = reqwest::get(&url).await?.text().await?;

                let problem = parse_html(year, problem, &html)?;

                Ok::<_, anyhow::Error>((html, problem))
            });
            handles.push(handle);
        }

        for handle in handles {
            let (html, problem) = handle.await??;
            content.problems.push(problem);
            content.problems.sort_by(|a, b| a.number.cmp(&b.number));

            if styles.is_empty() {
                styles = get_stylesheets(&html)?;
            }
        }
        Ok((content, styles))
    }
}

impl AopsContent {
    pub fn new(year: u32) -> Self {
        Self {
            year,
            problems: vec![],
        }
    }
}

impl AopsScrapeResult {
    pub fn generate_problem(&mut self) -> Result<String> {
        self.is_solution = false;
        Ok(self.render()?)
    }

    pub fn generate_solution(&mut self) -> Result<String> {
        self.is_solution = true;
        Ok(self.render()?)
    }
}

fn get_url(year: u32, problem: u32, challenge: Challenge) -> String {
    format!(
        "https://artofproblemsolving.com/wiki/index.php/{}_{}_Problems/Problem_{}",
        year, challenge, problem
    )
}

fn get_stylesheets(html: &str) -> Result<Vec<String>> {
    let fragment = Html::parse_document(html);
    let styles = fragment
        .select(&Selector::parse("link[rel=stylesheet]").unwrap())
        .filter_map(|node| {
            node.value().attr("href").and_then(|href| {
                if href.ends_with("css") {
                    Some(href.to_string())
                } else {
                    None
                }
            })
        })
        .collect::<Vec<_>>();

    Ok(styles)
}

fn parse_html(year: u32, number: u32, html: &str) -> Result<AopsProblem> {
    let fragment = Html::parse_document(html);
    let problem = fragment
        .select(&Selector::parse("div.mw-parser-output").unwrap())
        .next()
        .ok_or_else(|| anyhow::anyhow!("No problem found"))
        .with_context(|| format!("Failed to process {year}:{number}"))?;

    let mut fragment = Html::parse_fragment(problem.html().as_str());
    let node = fragment.select(&Selector::parse("div#toc").unwrap()).next();
    let mut has_toc = false;
    if let Some(node) = node {
        has_toc = true;
        fragment.remove_from_parent(&node.id());
    }

    let problem = parse_problem(&fragment, has_toc, false, year, number)?;
    let solution = parse_problem(&fragment, has_toc, true, year, number)?;

    Ok(AopsProblem {
        year,
        number,
        problem,
        solution,
    })
}

fn parse_problem(
    fragment: &Html,
    has_toc: bool,
    is_solution: bool,
    year: u32,
    number: u32,
) -> Result<String> {
    let mut fragment = fragment.clone();
    let mut node_to_delete = vec![];
    let mut start_to_delete = is_solution;

    let problem_pos = if has_toc { 1 } else { 0 };

    let node = get_solution_node(
        &fragment,
        &["#Solution", "#Solution_1", "#Solution_1_\\(Unrigorous\\)"],
    )
    .ok_or_else(|| anyhow::anyhow!("No solution found"))
    .with_context(|| format!("Failed to process {year}:{number}"))?
    .parent()
    .ok_or_else(|| anyhow::anyhow!("No solution parent found"))
    .with_context(|| format!("Failed to process {year}:{number}"))?;

    let see_also_node = fragment
        .select(&Selector::parse("#See_Also").unwrap())
        .next()
        .and_then(|node| node.parent());

    let parent = node
        .parent()
        .ok_or_else(|| anyhow::anyhow!("No parent found"))
        .with_context(|| format!("Failed to process {year}:{number}"))?;

    for (idx, child) in parent.children().enumerate() {
        if !is_solution && idx == problem_pos {
            node_to_delete.push(child.id());
        }
        if child.id() == node.id() {
            start_to_delete = !is_solution;
        }

        if see_also_node.is_some() && child.id() == see_also_node.unwrap().id() {
            start_to_delete = true;
        }

        if start_to_delete {
            node_to_delete.push(child.id());
        }
    }

    for id in node_to_delete {
        fragment.remove_from_parent(&id);
    }
    Ok(fragment.root_element().inner_html())
}

// ids: ["Solution", "Solution_1", "Solution_2"]
fn get_solution_node<'a>(fragment: &'a Html, ids: &[&str]) -> Option<ElementRef<'a>> {
    for id in ids {
        let node = fragment.select(&Selector::parse(id).unwrap()).next();
        if node.is_some() {
            return node;
        }
    }
    // fallback to use css selector. Solution should be the 2nd
    fragment
        .select(&Selector::parse("span.mw-headline").unwrap())
        .nth(1)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn parse_html_should_work() {
        let content = fs::read_to_string("fixtures/p23.html").unwrap();
        let styles = get_stylesheets(&content).unwrap();
        let result = parse_html(2003, 23, &content).unwrap();

        assert_eq!(result.year, 2003);
        assert_eq!(result.number, 23);

        insta::assert_yaml_snapshot!(styles);
    }

    #[test]
    fn render_problem_should_work() {
        let content = fs::read_to_string("fixtures/p23.html").unwrap();
        let result = parse_html(2003, 23, &content).unwrap();
        let mut ret = AopsScrapeResult {
            styles: get_stylesheets(&content).unwrap(),
            contents: vec![AopsContent {
                year: 2003,
                problems: vec![result],
            }],
            ..Default::default()
        };

        ret.generate_problem().unwrap();
    }

    #[test]
    fn render_2005p24_solution_should_work() {
        let content = fs::read_to_string("fixtures/2005p24.html").unwrap();
        let result = parse_html(2005, 24, &content).unwrap();
        let mut ret = AopsScrapeResult {
            styles: get_stylesheets(&content).unwrap(),
            contents: vec![AopsContent {
                year: 2005,
                problems: vec![result],
            }],
            ..Default::default()
        };

        ret.generate_solution().unwrap();
    }

    #[test]
    fn render_2009p22_solution_should_work() {
        let content = fs::read_to_string("fixtures/2009p22.html").unwrap();
        let result = parse_html(2009, 22, &content).unwrap();
        let mut ret = AopsScrapeResult {
            styles: get_stylesheets(&content).unwrap(),
            contents: vec![AopsContent {
                year: 2009,
                problems: vec![result],
            }],
            ..Default::default()
        };

        ret.generate_solution().unwrap();
    }
}
