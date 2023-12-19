use anyhow::Result;
use askama::Template;
use derive_builder::Builder;
use html5ever::tree_builder::TreeSink;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::ops::RangeInclusive;
use strum::{Display, EnumString};

#[derive(Debug, Builder, Serialize, Deserialize)]
pub struct AopsScraper {
    years: RangeInclusive<u32>,
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
    pub async fn scrape(&self) -> Result<AopsScrapeResult> {
        let mut styles = vec![];
        let mut contents = vec![];
        for year in self.years.clone() {
            let mut content = AopsContent::new(year);
            let mut handles = vec![];
            for problem in self.problems.clone() {
                let url = get_url(year, problem, self.challenge);
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

            contents.push(content);
        }

        Ok(AopsScrapeResult {
            styles,
            challenge: self.challenge,
            is_solution: false,
            contents,
        })
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
        .ok_or_else(|| anyhow::anyhow!("No problem found"))?;

    let mut fragment = Html::parse_fragment(problem.html().as_str());
    let node = fragment
        .select(&Selector::parse("div#toc").unwrap())
        .next()
        .ok_or_else(|| anyhow::anyhow!("No toc found"))?;
    fragment.remove_from_parent(&node.id());

    let problem = parse_problem(&fragment, false)?;
    let solution = parse_problem(&fragment, true)?;

    Ok(AopsProblem {
        year,
        number,
        problem,
        solution,
    })
}

fn parse_problem(fragment: &Html, is_solution: bool) -> Result<String> {
    let mut fragment = fragment.clone();
    let mut node_to_delete = vec![];
    let mut start_to_delete = is_solution;

    let node = fragment
        .select(&Selector::parse("#Solution").unwrap())
        .next()
        .or_else(|| {
            fragment
                .select(&Selector::parse("#Solution_1").unwrap())
                .next()
        })
        .ok_or_else(|| anyhow::anyhow!("No solution found"))?
        .parent()
        .ok_or_else(|| anyhow::anyhow!("No solution parent found"))?;

    let see_also_node = fragment
        .select(&Selector::parse("#See_Also").unwrap())
        .next()
        .and_then(|node| node.parent());

    let parent = node
        .parent()
        .ok_or_else(|| anyhow::anyhow!("No parent found"))?;

    for (idx, child) in parent.children().enumerate() {
        if !is_solution && idx == 1 {
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
}
