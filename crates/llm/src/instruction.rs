use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub enum Instruction {
    /// Standard Blocks (Normal Priority)
    Text(String),
    Item(String),
    NumberedItem(String),
    Section(Box<Instruction>, Vec<Instruction>),

    /// Important Blocks (High Priority)
    ImportantText(String),
    ImportantItem(String),
    ImportantNumberedItem(String),
    ImportantSection(Box<Instruction>, Vec<Instruction>),

    /// Raw Markdown injection
    Markdown(String),
}

impl Instruction {
    pub fn render(instructions: &[Instruction], depth: usize) -> String {
        Self::render_internal(instructions, 0, depth)
    }

    fn render_internal(
        instructions: &[Instruction],
        header_level: usize,
        indent_level: usize,
    ) -> String {
        let mut output = String::new();
        let mut numbered_count = 0;
        for (i, instruction) in instructions.iter().enumerate() {
            if i > 0 {
                output.push_str("\n\n");
            }

            if matches!(
                instruction,
                Instruction::NumberedItem(_) | Instruction::ImportantNumberedItem(_)
            ) {
                numbered_count += 1;
            } else {
                numbered_count = 0;
            }

            output.push_str(&instruction.render_single(header_level, indent_level, numbered_count));
        }
        output
    }

    fn render_single(
        &self,
        header_level: usize,
        indent_level: usize,
        numbered_index: usize,
    ) -> String {
        let indent = "  ".repeat(indent_level);
        match self {
            Instruction::Text(s) => format!("{}{}", indent, s),
            Instruction::Item(s) => format!("{}- {}", indent, s),
            Instruction::NumberedItem(s) => format!("{}{}. {}", indent, numbered_index, s),
            Instruction::Section(title, children) => {
                let mut output =
                    self.render_title(title, header_level, indent_level, false, numbered_index);

                let (next_header, next_indent) = if matches!(
                    **title,
                    Instruction::Text(_) | Instruction::ImportantText(_)
                ) {
                    (header_level + 1, indent_level)
                } else {
                    (header_level, indent_level + 1)
                };

                if !children.is_empty() {
                    output.push_str("\n\n");
                    output.push_str(&Self::render_internal(children, next_header, next_indent));
                }
                output
            }
            Instruction::ImportantText(s) => {
                format!("{}> [!IMPORTANT]\n{}> **{}**", indent, indent, s)
            }
            Instruction::ImportantItem(s) => {
                format!("{}- > [!IMPORTANT]\n{}  > **{}**", indent, indent, s)
            }
            Instruction::ImportantNumberedItem(s) => {
                format!(
                    "{}{}. > [!IMPORTANT]\n{}   > **{}**",
                    indent, numbered_index, indent, s
                )
            }
            Instruction::ImportantSection(title, children) => {
                let mut inner = self.render_title(title, header_level, 0, true, numbered_index);
                let (next_header, next_indent) = if matches!(
                    **title,
                    Instruction::Text(_) | Instruction::ImportantText(_)
                ) {
                    (header_level + 1, 0)
                } else {
                    (header_level, 1)
                };

                if !children.is_empty() {
                    inner.push_str("\n\n");
                    inner.push_str(&Self::render_internal(children, next_header, next_indent));
                }
                let mut output = format!("{}> [!IMPORTANT]\n", indent);
                for line in inner.lines() {
                    output.push_str(&format!("{}> {}\n", indent, line));
                }
                output.trim_end().to_string()
            }
            Instruction::Markdown(s) => format!("{}{}", indent, s),
        }
    }

    fn render_title(
        &self,
        title: &Instruction,
        header_level: usize,
        indent_level: usize,
        force_plain: bool,
        numbered_index: usize,
    ) -> String {
        match title {
            Instruction::Text(s) | Instruction::ImportantText(s) => {
                if force_plain {
                    format!("{} {}", "#".repeat(header_level + 1), s.to_uppercase())
                } else {
                    format!("{} {}", "#".repeat(header_level + 1), s)
                }
            }
            Instruction::Item(s) | Instruction::ImportantItem(s) => {
                format!("{}- {}", "  ".repeat(indent_level), s)
            }
            Instruction::NumberedItem(s) | Instruction::ImportantNumberedItem(s) => {
                format!("{}{}. {}", "  ".repeat(indent_level), numbered_index, s)
            }
            Instruction::Section(inner_title, _)
            | Instruction::ImportantSection(inner_title, _) => self.render_title(
                inner_title,
                header_level,
                indent_level,
                force_plain,
                numbered_index,
            ),
            Instruction::Markdown(s) => format!("{}{}", "  ".repeat(indent_level), s),
        }
    }
}
