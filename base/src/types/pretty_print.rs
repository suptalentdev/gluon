use std::borrow::Cow;

use pretty::{Arena, DocAllocator, DocBuilder};

use ast::{is_operator_char, Comment, CommentType};
use pos::{BytePos, Span};
use source::Source;

pub fn ident<'b, S>(arena: &'b Arena<'b>, name: S) -> DocBuilder<'b, Arena<'b>>
where
    S: Into<Cow<'b, str>>,
{
    let name = name.into();
    if name.starts_with(is_operator_char) {
        chain![arena; "(", name, ")"]
    } else {
        arena.text(name)
    }
}

pub fn doc_comment<'a>(
    arena: &'a Arena<'a>,
    text: Option<&'a Comment>,
) -> DocBuilder<'a, Arena<'a>> {
    match text {
        Some(comment) => match comment.typ {
            CommentType::Line => arena.concat(
                comment
                    .content
                    .lines()
                    .map(|line| arena.text("/// ").append(line).append(arena.newline())),
            ),
            CommentType::Block => chain![arena;
                        "/**",
                        arena.newline(),
                        arena.concat(comment.content.lines().map(|line| {
                            let line = line.trim();
                            if line.is_empty() {
                                arena.newline()
                            } else {
                                chain![arena;
                                    " ",
                                    line,
                                    arena.newline()
                                ]
                            }
                        })),
                        "*/",
                        arena.newline()
                    ],
        },
        None => arena.nil(),
    }
}

pub struct Printer<'a: 'e, 'e> {
    pub arena: &'a Arena<'a>,
    pub source: &'e Source<'a>,
}

impl<'a: 'e, 'e> Printer<'a, 'e> {
    pub fn new(arena: &'a Arena<'a>, source: &'e Source<'a>) -> Printer<'a, 'e> {
        Printer { arena, source }
    }

    pub fn space_before(&self, pos: BytePos) -> DocBuilder<'a, Arena<'a>> {
        let (doc, comments) = self.comments_before_(pos);
        if doc.1 == self.arena.nil().1 {
            self.arena.space()
        } else if comments {
            self.arena.space().append(doc).append(self.arena.space())
        } else {
            doc
        }
    }

    pub fn space_after(&self, end: BytePos) -> DocBuilder<'a, Arena<'a>> {
        let arena = self.arena;
        let doc = self.comments_after(end);
        if doc.1 == arena.nil().1 {
            arena.space()
        } else {
            arena.space().append(doc)
        }
    }

    pub fn comments_before(&self, pos: BytePos) -> DocBuilder<'a, Arena<'a>> {
        let (doc, comments) = self.comments_before_(pos);
        if comments {
            doc.append(self.arena.space())
        } else {
            doc
        }
    }

    fn comments_before_(&self, pos: BytePos) -> (DocBuilder<'a, Arena<'a>>, bool) {
        let arena = self.arena;
        let mut doc = arena.nil();
        let mut comments = 0;
        for comment in self.source.comments_between(Span::new(0.into(), pos)).rev() {
            let x = if comment.is_empty() {
                arena.newline()
            } else if comment.starts_with("//") {
                arena.text(comment).append(arena.newline())
            } else {
                comments += 1;
                arena.text(comment)
            };
            doc = x.append(doc);
        }
        (doc, comments != 0)
    }

    pub fn comments_after(&self, end: BytePos) -> DocBuilder<'a, Arena<'a>> {
        let (doc, block_comments, _) =
            self.comments_count(Span::new(end, self.source.src().len().into()));
        if block_comments == 0 {
            doc
        } else {
            let arena = self.arena;
            chain![arena;
                doc,
                arena.space()
            ]
        }
    }

    pub fn comments_count(&self, span: Span<BytePos>) -> (DocBuilder<'a, Arena<'a>>, usize, bool) {
        let arena = self.arena;
        let mut comments = 0;
        let mut ends_with_newline = false;
        let doc = arena.concat(self.source.comments_between(span).map(|comment| {
            ends_with_newline = false;
            if comment.is_empty() {
                ends_with_newline = true;
                arena.newline()
            } else if comment.starts_with("//") {
                arena.text(comment).append(arena.newline())
            } else {
                comments += 1;
                arena.text(comment)
            }
        }));
        (doc, comments, ends_with_newline)
    }
}
