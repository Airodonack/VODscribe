//! The specs of the search box's language, at the parser: `parse`, the
//! alternatives `Query::conjunctions` gives `Db::search`, `names`, `seconds`
//! and `QueryInfo`, what the page shows of a query. The parser is the spec,
//! so these test it as a function, a spec each, named as it is said. What a
//! query finds is search's (db/tests/search.rs).

use super::*;

fn loose(text: &str) -> Node {
    Node::Phrase { text: text.into(), strict: false, prefix: false }
}
fn typing(text: &str) -> Node {
    Node::Phrase { text: text.into(), strict: false, prefix: true }
}
fn strict(text: &str) -> Node {
    Node::Phrase { text: text.into(), strict: true, prefix: false }
}
fn not(item: Node) -> Node {
    Node::Not { item: Box::new(item) }
}
fn and(items: Vec<Node>) -> Node {
    Node::And { items }
}
fn or(items: Vec<Node>) -> Node {
    Node::Or { items }
}
fn filter(key: Key, value: &str) -> Node {
    Node::Filter { key, value: value.into() }
}
/// The grouping `text` parses to.
fn tree(text: &str) -> Node {
    parse(text).unwrap().tree.unwrap()
}
/// The message `text` is refused with.
fn error(text: &str) -> String {
    parse(text).unwrap_err()
}
/// The alternatives `text` is run as.
fn alternatives(text: &str) -> Vec<Conjunction> {
    parse(text).unwrap().conjunctions().unwrap()
}
/// `text`'s one alternative.
fn only(text: &str) -> Conjunction {
    let mut c = alternatives(text);
    assert_eq!(c.len(), 1, "{text} is one alternative");
    c.remove(0)
}
/// What the page shows of `text`.
fn info(text: &str) -> QueryInfo {
    QueryInfo::of(&parse(text).unwrap())
}
fn terms(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| w.to_string()).collect()
}

// Pieces and phrases.

/// "words typed one after another are one loose phrase in order"
/// (query.rs, the module's doc); decided for 0.6.0: unquoted words are one
/// loose phrase, in order.
#[test]
fn words_typed_one_after_another_are_one_loose_phrase_in_order() {
    assert_eq!(tree("clutch play"), typing("clutch play"));
    assert_eq!(tree("don't stop"), typing("don't stop"));
    assert_eq!(only("clutch play now").pieces, [Piece { terms: terms(&["clutch", "play", "now"]), prefix: true }]);
}

/// "the last word a prefix while it's being typed" (query.rs, the module's doc); "The
/// phrase the query ends with is the one being typed" (query.rs `Parser::atom`);
/// decided for 0.6.0: the last word is a prefix while typing, once it has
/// two letters or more.
#[test]
fn the_last_word_is_a_prefix_while_it_is_being_typed() {
    assert_eq!(tree("clutch pl"), typing("clutch pl"));
    assert_eq!(tree("clutch pl "), typing("clutch pl"), "a space after doesn't end the typing");
    assert!(only("clutch pl").pieces[0].prefix);
}

/// "The phrase the query ends with is the one being typed" (query.rs `Parser::atom`).
#[test]
fn only_the_phrase_the_query_ends_with_is_being_typed() {
    assert_eq!(tree("clutch pl \"good\""), and(vec![loose("clutch pl"), strict("good")]));
    assert_eq!(tree("clutch pl from:bob"), and(vec![loose("clutch pl"), filter(Key::From, "bob")]));
    assert_eq!(tree("clutch OR play"), or(vec![loose("clutch"), typing("play")]));
    assert!(!only("clutch pl in:chat").pieces[0].prefix);
}

/// 0.2.1: "A one-letter search now finds that word, rather than every word
/// starting with the letter."; decided for 0.6.0: a prefix needs two
/// letters or more.
#[test]
fn a_one_letter_last_word_is_that_word_not_a_prefix() {
    assert_eq!(tree("a"), loose("a"));
    assert_eq!(tree("clutch p"), loose("clutch p"));
    assert!(!only("clutch p").pieces[0].prefix);
}

/// "a quoted string a strict phrase of whole words" (query.rs, the module's doc);
/// decided for 0.6.0: a quoted string is a strict phrase, whole words, no
/// prefix.
#[test]
fn a_quoted_string_is_a_strict_phrase_of_whole_words() {
    assert_eq!(tree("\"a good shot\""), strict("a good shot"));
    assert_eq!(tree("hi \"clutch\""), and(vec![loose("hi"), strict("clutch")]), "never a prefix, even last");
    assert_eq!(only("hi \"clutch\"").pieces[1], Piece { terms: terms(&["clutch"]), prefix: false });
}

/// Decided for 0.6.0: words form a phrase until a quote, an operator, a
/// bracket or a `key:` splits them, so `well that was OR damn` is `"well
/// that was" OR "damn"`, and `nice "clutch"` is the loose phrase "nice" and
/// the word "clutch"; 0.6.0: "the two mix: `nice "clutch"` finds "nice"
/// with "clutch" anywhere near it".
#[test]
fn words_form_a_phrase_until_a_quote_an_operator_a_bracket_or_a_filter() {
    assert_eq!(tree("nice \"clutch\""), and(vec![loose("nice"), strict("clutch")]));
    assert_eq!(tree("well that was OR damn"), or(vec![loose("well that was"), typing("damn")]));
    assert_eq!(tree("a b AND c"), and(vec![loose("a b"), loose("c")]));
    assert_eq!(tree("nice (clutch) play"), and(vec![loose("nice"), loose("clutch"), typing("play")]));
    assert_eq!(tree("nice from:x play"), and(vec![loose("nice"), filter(Key::From, "x"), typing("play")]));
    assert_eq!(tree("nice -b play"), and(vec![loose("nice"), not(loose("b")), typing("play")]));
}

/// 0.6.0: "Quoted words are found in any order. They need only all be said
/// within 20 seconds of each other, as in `"all" "words" "any" "order"`";
/// the window is search's. Here: each is a piece of its own.
#[test]
fn quoted_words_are_pieces_of_their_own() {
    assert_eq!(
        tree("\"all\" \"words\" \"any\" \"order\""),
        and(vec![strict("all"), strict("words"), strict("any"), strict("order")])
    );
    let pieces: Vec<Vec<String>> =
        only("\"all\" \"words\" \"any\" \"order\"").pieces.into_iter().map(|p| p.terms).collect();
    assert_eq!(pieces, [terms(&["all"]), terms(&["words"]), terms(&["any"]), terms(&["order"])]);
}

/// Decided for 0.6.0, the language's worked examples: `well that was a good
/// shot "damn"` (a phrase over three lines and a word near it), the same in
/// brackets, `i did it -frick`, and `well that was OR damn`; 0.6.0 names the
/// last two too.
#[test]
fn the_worked_examples_read_as_decided() {
    assert_eq!(
        tree("well that was a good shot \"damn\""),
        and(vec![loose("well that was a good shot"), strict("damn")])
    );
    assert_eq!(tree("(well that was a good shot \"damn\")"), tree("well that was a good shot \"damn\""));
    assert_eq!(tree("i did it -frick"), and(vec![loose("i did it"), not(loose("frick"))]));
    assert_eq!(tree("well that was OR damn"), or(vec![loose("well that was"), typing("damn")]));
}

/// Piece: "its words folded as the index folds them" (query.rs `Piece`).
#[test]
fn a_querys_words_are_folded_as_the_index_folds_them() {
    let c = only("Café don't");
    assert_eq!(c.pieces, [Piece { terms: terms(&["cafe", "don", "t"]), prefix: false }]);
    assert!(c.pieces[0].terms.iter().all(|t| t.chars().all(char::is_alphanumeric)), "nothing FTS reads");
}

/// Query: "None for a query with nothing in it" (query.rs `Query::tree`).
#[test]
fn a_query_with_nothing_in_it_has_no_tree() {
    assert_eq!(parse("").unwrap().tree, None);
    assert_eq!(parse("  !! - ").unwrap().tree, None);
    assert_eq!(parse("()").unwrap().tree, None);
    assert_eq!(parse("-!!").unwrap().tree, None);
    assert_eq!(tree("\"\" hello"), typing("hello"), "an empty quote is nothing");
    assert!(parse("").unwrap().conjunctions().unwrap().is_empty());
}

/// "Anything else is words." (query.rs, the module's doc); e2e
/// chat-search.mjs: "`@Ada` is words to find (a mention), not a filter".
#[test]
fn anything_else_is_words() {
    assert_eq!(tree("re:zero"), typing("re:zero"), "an unknown key");
    assert_eq!(tree("cats or dogs"), typing("cats or dogs"), "or in lower case");
    assert_eq!(tree("cats and dogs"), typing("cats and dogs"), "and in lower case");
    assert_eq!(tree("at 12:30"), typing("at 12:30"));
    assert_eq!(tree("@Ada hi"), typing("@Ada hi"));
    let c = only("@Ada hi");
    assert!(c.from.is_empty() && !c.filtered());
    assert_eq!(c.pieces[0].terms, terms(&["ada", "hi"]));
}

// Minus.

/// "`-word` or `-"a phrase"` something that mustn't be near" (query.rs, the module's doc);
/// 0.6.0: "A minus leaves out matches. Put it before a word or a quoted
/// phrase to leave out matches with it nearby, as in `i did it -frick`."
#[test]
fn a_minus_leaves_out_a_word_or_a_quoted_phrase() {
    assert_eq!(tree("i did it -frick"), and(vec![loose("i did it"), not(loose("frick"))]));
    assert_eq!(tree("-\"a phrase\" bb"), and(vec![not(strict("a phrase")), typing("bb")]));
    let c = only("i did it -frick -\"a phrase\"");
    assert_eq!(c.pieces, [Piece { terms: terms(&["i", "did", "it"]), prefix: false }]);
    assert_eq!(
        c.not,
        [Piece { terms: terms(&["frick"]), prefix: false }, Piece { terms: terms(&["a", "phrase"]), prefix: false }]
    );
}

/// 0.6.0: "Put it before a word or a quoted phrase"; and "a search
/// VODscribe can't read … says what to fix".
#[test]
fn a_minus_goes_only_before_a_word_or_a_quoted_phrase() {
    assert!(error("-(a b)").contains("minus"));
    assert!(error("-from:bob").contains("minus"));
}

/// "Characters that end a word: brackets and quotes stand on their own"
/// (query.rs `ends_word`); "A dash on its own is punctuation." (query.rs `tokens`).
#[test]
fn a_dash_inside_a_word_is_the_words_and_one_on_its_own_is_punctuation() {
    assert_eq!(tree("well-known"), typing("well-known"));
    assert_eq!(tree("hello -"), typing("hello"));
    assert_eq!(tree("hello - there"), typing("hello there"));
}

/// Decided for 0.6.0: exclusions alone, with no filter and no piece, find
/// nothing.
#[test]
fn exclusions_alone_look_for_nothing() {
    let c = only("-frick");
    assert!(c.pieces.is_empty() && !c.filtered());
}

// OR, AND and brackets.

/// "`OR` offers alternatives" (query.rs, the module's doc); 0.6.0: "OR finds either side.";
/// `Query::conjunctions`: "the alternatives `Db::search` runs, one by one"
/// (query.rs, the module's doc).
#[test]
fn or_offers_alternatives() {
    assert_eq!(tree("\"a\" OR \"b\""), or(vec![strict("a"), strict("b")]));
    let c = alternatives("\"a\" OR \"b\"");
    assert_eq!(c.iter().map(|c| c.pieces[0].terms.clone()).collect::<Vec<_>>(), [terms(&["a"]), terms(&["b"])]);
}

/// "`AND` joins pieces explicitly, binding tighter than `OR`" (query.rs, the module's doc);
/// 0.6.0: "AND binds more tightly than OR".
#[test]
fn and_binds_tighter_than_or() {
    assert_eq!(tree("\"a\" AND \"b\" OR \"c\""), or(vec![and(vec![strict("a"), strict("b")]), strict("c")]));
    assert_eq!(tree("\"a\" OR \"b\" AND \"c\""), or(vec![strict("a"), and(vec![strict("b"), strict("c")])]));
    assert_eq!(alternatives("\"a\" AND \"b\" OR \"c\"").iter().map(|c| c.pieces.len()).collect::<Vec<_>>(), [2, 1]);
}

/// "brackets group" (query.rs, the module's doc); 0.6.0: "brackets group".
#[test]
fn brackets_group() {
    assert_eq!(tree("(\"a\" OR \"b\") \"c\""), and(vec![or(vec![strict("a"), strict("b")]), strict("c")]));
    assert_eq!(tree("\"a\" AND (\"b\" OR \"c\")"), and(vec![strict("a"), or(vec![strict("b"), strict("c")])]));
    let c = alternatives("(\"a\" OR \"b\") \"c\"");
    assert_eq!(c.len(), 2);
    assert!(c.iter().all(|c| c.pieces.len() == 2 && c.pieces[1].terms == ["c"]));
}

/// "Pieces and filters side by side (the implicit AND) bind looser than
/// `OR`, so `from:x "a" OR "b"` is from x, a or b." (query.rs, the module's doc);
/// the support page (vodscribe.com/support, "How do I search?"): "Filters
/// apply to the whole search: `from:x "a" OR "b"` is a or b, both from x."
#[test]
fn pieces_and_filters_side_by_side_bind_looser_than_or() {
    assert_eq!(tree("from:x \"a\" OR \"b\""), and(vec![filter(Key::From, "x"), or(vec![strict("a"), strict("b")])]));
    assert_eq!(
        tree("nice \"clutch\" OR \"play\" \"good\""),
        and(vec![loose("nice"), or(vec![strict("clutch"), strict("play")]), strict("good")])
    );
    assert!(alternatives("from:x \"a\" OR \"b\"").iter().all(|c| c.from == ["x"]));
}

/// "Items with any of the same kind inside merged into them (`(a AND b) AND
/// c` is `a AND b AND c`)" (query.rs `group`).
#[test]
fn a_group_inside_one_of_its_own_kind_is_part_of_it() {
    assert_eq!(tree("(\"a\" AND \"b\") AND \"c\""), and(vec![strict("a"), strict("b"), strict("c")]));
    assert_eq!(tree("(\"a\" OR \"b\") OR \"c\""), or(vec![strict("a"), strict("b"), strict("c")]));
    assert_eq!(tree("((\"a\"))"), strict("a"));
}

/// "An OR or AND with nothing on one side (while typing `a OR`) is left
/// out." (query.rs `Parser::skip_operators`); "`a AND OR b`: the OR is the one meant."
/// (query.rs `Parser::and`).
#[test]
fn an_or_or_and_with_nothing_on_one_side_is_left_out() {
    assert_eq!(tree("well that was OR"), loose("well that was"));
    assert_eq!(tree("OR damn"), typing("damn"));
    assert_eq!(tree("a OR OR bb"), or(vec![loose("a"), typing("bb")]));
    assert_eq!(tree("a AND"), loose("a"));
    assert_eq!(tree("AND a"), loose("a"));
    assert_eq!(tree("\"a\" AND OR \"b\""), or(vec![strict("a"), strict("b")]));
}

/// "More alternatives than this (as `(a OR b) (c OR d) …` multiplies them)
/// is refused rather than run one by one." (query.rs `MOST_ALTERNATIVES`, 64 of them).
#[test]
fn more_than_64_alternatives_are_refused() {
    let groups = |n: usize| (0..n).map(|i| format!("(\"a{i}\" OR \"b{i}\")")).collect::<Vec<_>>().join(" ");
    assert_eq!(alternatives(&groups(6)).len(), 64);
    assert!(parse(&groups(7)).unwrap().conjunctions().unwrap_err().contains("alternatives"));
    // `n` words, each an alternative, from `a` on.
    let ors = |a: char, n: usize| (0..n).map(|i| format!("\"{a}{i}\"")).collect::<Vec<_>>().join(" OR ");
    assert_eq!(alternatives(&ors('a', 64)).len(), 64);
    assert!(parse(&ors('a', 65)).unwrap().conjunctions().unwrap_err().contains("alternatives"));
    // 8 by 9 is 72 of them.
    let both = format!("({}) ({})", ors('a', 8), ors('b', 9));
    assert!(parse(&both).unwrap().conjunctions().unwrap_err().contains("alternatives"));
}

/// "Brackets inside each other deeper than this are refused" (query.rs `MOST_BRACKETS`,
/// 32 of them): thousands pasted ran the parser out of stack, which aborts
/// the app.
#[test]
fn brackets_deep_inside_each_other_are_refused() {
    let nested = |n: usize| format!("{}a{}", "(".repeat(n), ")".repeat(n));
    assert_eq!(alternatives(&nested(MOST_BRACKETS)).len(), 1);
    assert!(error(&nested(MOST_BRACKETS + 1)).contains("brackets"));
    assert!(error(&nested(100_000)).contains("brackets"));
    assert!(error(&"(".repeat(100_000)).contains("brackets"));
    // Side by side, not inside each other: any number.
    let side_by_side = (0..100).map(|i| format!("(w{i})")).collect::<Vec<_>>().join(" ");
    assert_eq!(alternatives(&side_by_side).len(), 1);
}

// Filters.

/// "`from:name`" (query.rs, the module's doc); 0.6.0: "Type `from:name` for what a channel
/// said and wrote or a chatter wrote (`from:"Some One"` for a name with a
/// space …)".
#[test]
fn from_names_who_said_or_wrote_it() {
    assert_eq!(tree("from:bob"), filter(Key::From, "bob"));
    assert_eq!(only("from:bob hi").from, ["bob"]);
    assert_eq!(tree("from:\"Some One\" hi"), and(vec![filter(Key::From, "Some One"), typing("hi")]));
    assert_eq!(only("from:\"Some One\" hi").from, ["Some One"]);
}

/// "`from:a,b` for either" (query.rs, the module's doc); 0.6.0: "`from:ada,marlowe` for
/// several"; `from_value`: "one name, or several separated by commas
/// (`from:a,b`), each bare or in quotes for one with a space or a comma
/// (`from:a,"Some One"`)" (query.rs `from_value`).
#[test]
fn from_takes_several_names_separated_by_commas_any_in_quotes() {
    assert_eq!(tree("from:a,b"), filter(Key::From, "a,b"));
    assert_eq!(only("from:ada,marlowe").from, ["ada", "marlowe"]);
    assert_eq!(only("from:a,\"Some One\",c").from, ["a", "Some One", "c"]);
    assert_eq!(only("from:\"Some One\",a").from, ["Some One", "a"]);
    assert_eq!(tree("from:a,\"Some One\" hi"), and(vec![filter(Key::From, "a,\"Some One\""), typing("hi")]));
}

/// `names`: "a comma inside quotes is the name's" (query.rs `names`).
#[test]
fn a_comma_inside_quotes_is_the_names() {
    assert_eq!(only("from:\"Last, First\"").from, ["Last, First"]);
    assert_eq!(only("from:\"Last, First\",bob").from, ["Last, First", "bob"]);
    assert_eq!(names("\"Last, First\", b"), ["Last, First", "b"]);
}

/// "Every name `from:` gives shows, as any of them is searched for"
/// (query.rs `QueryInfo::of`); Conjunction.from: "Who: any of these names" (query.rs `Conjunction::from`).
#[test]
fn from_typed_twice_is_any_of_their_names() {
    assert_eq!(only("from:a from:b,c hi").from, ["a", "b", "c"]);
}

/// `names`: "The names in a `from:` value …, or in the filter's From whom:
/// one, or several separated by commas, any of them in quotes"
/// (query.rs `names`); 0.6.0: the From whom filter "takes several separated by
/// commas".
#[test]
fn names_reads_the_from_whom_controls_text() {
    assert_eq!(names("Ada"), ["Ada"]);
    assert_eq!(names(" Ada ,  Some One"), ["Ada", "Some One"]);
    assert_eq!(names("\"Some One\",bob"), ["Some One", "bob"]);
}

/// "`in:chat` / `in:audio`" (query.rs, the module's doc); 0.6.0: "`in:chat` or
/// `in:audio`"; Conjunction.sources: "Typed
/// `in:`, which takes the place of the filter's sources" (query.rs `Conjunction::sources`).
#[test]
fn in_chat_or_in_audio_chooses_the_sources() {
    assert_eq!(only("in:chat hi").sources, Some(Sources { audio: false, chat: true }));
    assert_eq!(only("in:audio hi").sources, Some(Sources { audio: true, chat: false }));
    assert_eq!(only("in:chat in:audio hi").sources, Some(Sources { audio: true, chat: true }), "either in:");
    assert_eq!(only("hi").sources, None, "the filter's own");
}

/// 0.6.0: "`in:chat` or `in:audio`", and "a search VODscribe can't read …
/// says what to fix".
#[test]
fn in_takes_only_audio_or_chat() {
    assert!(error("in:video").contains("in:audio"));
}

/// Decided for 0.6.0: `within:20`, `within:20s`, `within:2m` and
/// `within:1h`, a bare number being seconds; 0.6.0:
/// "`within:2m` for how close together the words must be (seconds, minutes
/// or hours …)".
#[test]
fn within_takes_seconds_minutes_or_hours_a_bare_number_seconds() {
    let within = |text: &str| parse(text).unwrap().within;
    assert_eq!(within("gg within:20"), Some(20.0));
    assert_eq!(within("gg within:20s"), Some(20.0));
    assert_eq!(within("gg within:2m"), Some(120.0));
    assert_eq!(within("gg within:1h"), Some(3600.0));
}

/// Decided for 0.6.0: `within:` anywhere in the query is the whole query's,
/// the last one wins, and it shows in the filter's control; Query.within:
/// "the last `within:` anywhere in the
/// query" (query.rs `Query::within`).
#[test]
fn the_last_within_anywhere_in_the_query_is_the_window() {
    assert_eq!(parse("within:5 a within:2m").unwrap().within, Some(120.0));
    assert_eq!(parse("(a within:2m) OR b").unwrap().within, Some(120.0), "inside an alternative too");
    assert_eq!(info("(a within:2m) OR b").within, Some(120.0), "shown in the filter's control");
}

/// Query.within: "The window, if typed" (query.rs `Query::within`); 0.6.0:
/// "20 seconds unless you say" (the default is search's).
#[test]
fn within_not_typed_leaves_the_window_to_the_filter() {
    assert_eq!(parse("gg").unwrap().within, None);
    assert_eq!(info("gg from:bob").within, None);
}

/// `MOST_WITHIN`: "The widest window, typed or the filter's: an hour."
/// (query.rs `MOST_WITHIN`); the support page: "`within:20s` … at most an hour".
#[test]
fn within_is_at_most_an_hour() {
    assert_eq!(parse("chat within:1h").unwrap().within, Some(MOST_WITHIN));
    assert_eq!(parse("chat within:60m").unwrap().within, Some(MOST_WITHIN));
    assert_eq!(error("chat within:1h0m1s"), TOO_WIDE);
    assert_eq!(error("chat within:3h"), TOO_WIDE);
    assert_eq!(error("(a within:2h) OR b"), TOO_WIDE);
}

/// `parse`: "a filter's value that isn't one, is the message to show"
/// (query.rs `parse`); 0.6.0: "a search VODscribe can't read … says what to
/// fix".
#[test]
fn a_within_that_is_not_a_length_of_time_says_so() {
    assert!(error("within:soon").contains("within:"));
}

/// 0.6.0: "`after:1:20:00` or `before:2h`
/// for where in the VOD"; Conjunction: "A match starts at or after `after`
/// and before `before`, seconds into the VOD" (query.rs `Conjunction::after`).
#[test]
fn after_and_before_take_a_time_into_the_vod() {
    let c = only("gg after:1:20:00 before:2h");
    assert_eq!((c.after, c.before), (Some(4800.0), Some(7200.0)));
    assert_eq!(only("gg after:90m").after, Some(5400.0));
    let shown = info("gg after:1:20:00 before:2h");
    assert_eq!((shown.after, shown.before), (Some(4800.0), Some(7200.0)));
}

/// "Both hold: the later start and the earlier end." (query.rs `Conjunction::and`); "times
/// typed twice all hold (`Conjunction::and`), so the latest start and the
/// earliest end show, as the search applies them" (query.rs `QueryInfo::of`).
#[test]
fn after_or_before_typed_twice_both_hold() {
    let c = only("in:chat after:10 after:20 before:50 before:40");
    assert_eq!((c.after, c.before), (Some(20.0), Some(40.0)));
}

/// `parse`: "a filter's value that isn't one, is the message to show"
/// (query.rs `parse`).
#[test]
fn a_time_that_is_not_one_says_so() {
    assert!(error("after:yesterday").contains("after:"));
    assert!(error("before:soon").contains("before:"));
}

/// Node::Filter: "`value` as typed; empty while it's being typed, and then
/// it does nothing." (query.rs `Node::Filter`).
#[test]
fn a_filter_being_typed_does_nothing_yet() {
    assert_eq!(tree("from:"), filter(Key::From, ""));
    for text in ["hi from:", "hi in:", "hi within:", "hi after:", "hi before:"] {
        let query = parse(text).unwrap();
        assert_eq!(query.within, None, "{text}");
        let hi = Piece { terms: terms(&["hi"]), prefix: false };
        assert_eq!(query.conjunctions().unwrap(), [Conjunction { pieces: vec![hi], ..Default::default() }], "{text}");
        assert_eq!(QueryInfo::of(&query), QueryInfo { tree: query.tree.clone(), ..Default::default() }, "{text}");
    }
    assert_eq!(only("from: hi").from, Vec::<String>::new());
}

/// "A quoted value: within:"2m"." (query.rs `tokens`); 0.6.0: "`from:"Some
/// One"` for a name with a space".
#[test]
fn a_filters_value_may_be_quoted() {
    assert_eq!(parse("gg within:\"2m\"").unwrap().within, Some(120.0));
    assert_eq!(only("gg after:\"1:20:00\"").after, Some(4800.0));
    assert_eq!(only("gg in:\"chat\"").sources, Some(Sources { audio: false, chat: true }));
}

/// Decided for 0.6.0: filters alone list everything they pass, as
/// `from:bob` and `from:bob -hello`; 0.6.0: `from:name` "alone it lists everything they said and
/// wrote"; `Conjunction::filtered`: "with no pieces it still lists what
/// passes" (query.rs `Conjunction::filtered`); e2e chat-search.mjs: "`in:chat` alone lists
/// chat".
#[test]
fn a_filter_alone_lists_everything_it_passes() {
    for text in ["from:bob", "from:bob -hello", "in:chat", "after:10", "before:1h"] {
        let c = only(text);
        assert!(c.pieces.is_empty() && c.filtered(), "{text}");
    }
    assert_eq!(only("from:bob -hello").not, [Piece { terms: terms(&["hello"]), prefix: false }]);
}

/// 0.6.0: "`(from:alice "hi") OR (from:bob "gg")`"; decided for 0.6.0:
/// filters inside a group apply to that alternative only;
/// `Query::conjunctions`: "a filter inside brackets is that alternative's"
/// (query.rs `Query::conjunctions`).
#[test]
fn alternatives_carry_their_own_filters() {
    assert_eq!(
        tree("(from:a \"x\") OR (from:b \"y\")"),
        or(vec![and(vec![filter(Key::From, "a"), strict("x")]), and(vec![filter(Key::From, "b"), strict("y")])])
    );
    let c = alternatives("(from:a \"x\") OR (from:b \"y\")");
    assert_eq!(
        c.iter().map(|c| (c.from.clone(), c.pieces[0].terms.clone())).collect::<Vec<_>>(),
        [(terms(&["a"]), terms(&["x"])), (terms(&["b"]), terms(&["y"]))]
    );
    let c = alternatives("(in:chat after:10 \"x\") OR \"y\"");
    assert_eq!((c[0].sources, c[0].after), (Some(Sources { audio: false, chat: true }), Some(10.0)));
    assert_eq!((c[1].sources, c[1].after), (None, None));
}

/// "`from:x "a" OR "b"` is from x, a or b." (query.rs, the module's doc).
#[test]
fn a_filter_outside_brackets_holds_for_every_alternative() {
    let c = alternatives("from:x (\"a\" OR \"b\") \"c\" -d");
    assert_eq!(c.len(), 2);
    assert!(c.iter().all(|c| c.from == ["x"] && c.pieces.len() == 2 && c.not.len() == 1));
}

// Errors.

/// 0.6.0: "a search VODscribe can't read, such as one with a quote left
/// open, says what to fix there instead"; `parse`: "an unbalanced quote …
/// is the message to show" (query.rs `parse`).
#[test]
fn a_quote_left_open_says_to_close_it() {
    for text in ["\"unclosed", "-\"unclosed", "\" OR *", "from:a,\"Some", "within:\"2m"] {
        assert!(error(text).contains("quote"), "{text}");
    }
}

/// `parse`: "an unbalanced … bracket … is the message to show"
/// (query.rs `parse`); 0.6.0: "says what to fix".
#[test]
fn a_bracket_left_open_or_closed_unopened_says_so() {
    assert!(error("(a OR b").contains("bracket isn't closed"));
    assert!(error("((a) b").contains("bracket isn't closed"));
    assert!(error("a OR b)").contains("wasn't opened"));
}

// What the page shows.

/// QueryInfo: "What the typed text set, as the page shows it: the grouping,
/// and the filters that apply to the whole query" (query.rs `QueryInfo`);
/// `top_filters`: "those not inside an `OR` or brackets" (query.rs `Query::top_filters`).
#[test]
fn the_filters_shown_are_those_of_the_whole_query() {
    let shown = info("from:x in:chat after:10 before:20 (\"a\" OR \"b\")");
    assert_eq!(shown.from.as_deref(), Some("x"));
    assert_eq!(shown.sources, Some(Sources { audio: false, chat: true }));
    assert_eq!((shown.after, shown.before), (Some(10.0), Some(20.0)));
    let shown = info("(from:a in:chat after:10 \"x\") OR (from:b before:20 \"y\")");
    assert_eq!((shown.from, shown.sources, shown.after, shown.before), (None, None, None, None));
}

/// QueryInfo: "the filter's controls then show as set by the text"
/// (query.rs `QueryInfo`); "times typed twice all hold …, so the latest start and
/// the earliest end show, as the search applies them" (query.rs `QueryInfo::of`).
#[test]
fn the_filters_shown_are_the_ones_applied() {
    let query = parse("gg after:20 after:10 before:40 before:50 in:chat in:audio").unwrap();
    let shown = QueryInfo::of(&query);
    let c = &query.conjunctions().unwrap()[0];
    assert_eq!((shown.after, shown.before, shown.sources), (c.after, c.before, c.sources));
    assert_eq!((shown.after, shown.before), (Some(20.0), Some(40.0)));
}

/// QueryInfo.from: "Every name `from:` gives, each once, separated by ", "
/// (one with a comma in quotes), as the From whom control takes them"
/// (query.rs `QueryInfo::from`); decided for 0.6.0: `from:` matches a name
/// in any case, so one name in either case is the same name.
#[test]
fn the_from_whom_control_shows_every_name_once() {
    assert_eq!(info("from:a,\"Some One\" hi").from.as_deref(), Some("a, Some One"));
    assert_eq!(info("from:Ada from:bob,ada").from.as_deref(), Some("Ada, bob"));
    assert_eq!(info("from:\"Some One\"").from.as_deref(), Some("Some One"));
    assert_eq!(info("from:\"Last, First\",b").from.as_deref(), Some("\"Last, First\", b"));
    assert_eq!(info("hi").from, None);
    // The control's text, read back, is the same names.
    assert_eq!(names(&info("from:\"Last, First\",b").from.unwrap()), ["Last, First", "b"]);
}

/// QueryInfo: "`error` (an unbalanced quote, say) comes with no hits"
/// (query.rs `QueryInfo`); 0.6.0: a search it can't read "says what to fix there
/// instead" (of the chips).
#[test]
fn a_query_that_cannot_be_read_shows_its_message_in_place_of_the_grouping() {
    let message = error("\"unclosed");
    let shown = QueryInfo::error(message.clone());
    assert_eq!(shown, QueryInfo { error: Some(message), ..Default::default() });
}

/// What the page reads (api.ts `QueryNode` and `SearchQuery`): the tree as `{kind:"or",
/// items}`, `{kind:"and", items}`, `{kind:"phrase", text, strict, prefix}`,
/// `{kind:"not", item}` or `{kind:"filter", key, value}`, with `within`,
/// `from`, `sources` (`{audio, chat}` or null), `after`, `before` and `error`.
#[test]
fn the_query_as_the_page_reads_it() {
    let json = serde_json::to_value(tree("from:x (\"a\" OR -b) c")).unwrap();
    assert_eq!(
        json,
        serde_json::json!({"kind": "and", "items": [
            {"kind": "filter", "key": "from", "value": "x"},
            {"kind": "or", "items": [
                {"kind": "phrase", "text": "a", "strict": true, "prefix": false},
                {"kind": "not", "item": {"kind": "phrase", "text": "b", "strict": false, "prefix": false}}
            ]},
            {"kind": "phrase", "text": "c", "strict": false, "prefix": false}
        ]})
    );
    let mut json = serde_json::to_value(info("from:x in:chat within:2m after:10 before:20 hi")).unwrap();
    json.as_object_mut().unwrap().remove("tree");
    assert_eq!(
        json,
        serde_json::json!({"within": 120.0, "from": "x", "sources": {"audio": false, "chat": true},
            "after": 10.0, "before": 20.0, "error": null})
    );
    assert_eq!(
        serde_json::to_value([Key::From, Key::In, Key::Within, Key::After, Key::Before]).unwrap(),
        serde_json::json!(["from", "in", "within", "after", "before"])
    );
}

/// Node's Display: "Back to text that parses to the same tree: brackets
/// around every group inside another, and AND between two loose phrases
/// (side by side they would run into one)." (query.rs `Node`'s `Display`).
#[test]
fn shown_as_text_it_parses_the_same() {
    for text in [
        "well that was a good shot \"damn\"",
        "i did it -frick",
        "\"all\" \"words\" \"any\" \"order\"",
        "from:x \"a\" OR \"b\"",
        "\"a\" AND \"b\" OR \"c\"",
        "(from:a \"x\") OR (from:b \"y\")",
        "a b AND c",
        "nice \"clutch\" OR \"play\" good",
        "from:\"Some One\" -\"a phrase\" within:2m",
        "from:a,\"Some One\" hi",
        "from:\"Last, First\" hi",
        "from:\"Last, First\",b hi",
    ] {
        let parsed = tree(text);
        let shown = parsed.to_string();
        // Shown, the last phrase isn't being typed any more; else the same.
        assert_eq!(not_typing(tree(&format!("{shown} ()"))), not_typing(parsed), "{text} → {shown}");
    }
    assert_eq!(tree("from:x \"a\" OR \"b\"").to_string(), "from:x (\"a\" OR \"b\")");
}

/// `node` with no phrase being typed.
fn not_typing(node: Node) -> Node {
    match node {
        Node::Phrase { text, strict, .. } => Node::Phrase { text, strict, prefix: false },
        Node::Or { items } => Node::Or { items: items.into_iter().map(not_typing).collect() },
        Node::And { items } => Node::And { items: items.into_iter().map(not_typing).collect() },
        n => n,
    }
}

// Times.

/// `seconds`: "Seconds from `1:20:00`, `20:00`, `90`, `90s`, `2m`, `1h` or
/// `1h30m`; a bare number is seconds." (query.rs `seconds`).
#[test]
fn seconds_reads_each_form_it_names() {
    assert_eq!(seconds("1:20:00"), Some(4800.0));
    assert_eq!(seconds("20:00"), Some(1200.0));
    assert_eq!(seconds("90"), Some(90.0));
    assert_eq!(seconds("90s"), Some(90.0));
    assert_eq!(seconds("2m"), Some(120.0));
    assert_eq!(seconds("1h"), Some(3600.0));
    assert_eq!(seconds("1h30m"), Some(5400.0));
    assert_eq!(seconds("1h30m15s"), Some(5415.0));
}

/// "Numbers each followed by a unit, larger units first: 1h30m."
/// (query.rs `seconds`).
#[test]
fn seconds_takes_larger_units_first() {
    assert_eq!(seconds("30m1h"), None);
    assert_eq!(seconds("1m1m"), None);
}

/// `seconds`: the forms it names (query.rs `seconds`), and nothing else.
#[test]
fn seconds_reads_nothing_else_as_a_time() {
    for text in ["", " ", "2x", "h", "m30", "1:2:3:4", "1::2", "1h 30m", "-5", "soon"] {
        assert_eq!(seconds(text), None, "{text:?}");
    }
}

// Unstated: no source states these. They are kept as the tests before
// these had them, until it is decided whether they are specs.

/// Unstated: keys, and a time's units, are read in any case.
#[test]
fn unstated_keys_and_units_in_any_case() {
    assert_eq!(tree("FROM:Bob"), filter(Key::From, "Bob"));
    assert_eq!(seconds("1H"), Some(3600.0));
}

/// Unstated: a time's numbers may have decimals.
#[test]
fn unstated_a_time_may_have_decimals() {
    assert_eq!(seconds("1.5m"), Some(90.0));
}

/// Unstated: a comma with no name after it names no one (while
/// typing `from:a,`).
#[test]
fn unstated_an_empty_name_between_commas_is_no_one() {
    assert_eq!(only("from:a, hi").from, ["a"]);
    assert!(only("from:,").from.is_empty());
    assert_eq!(names(" Ada ,  Some One,"), ["Ada", "Some One"]);
}

/// Unstated: a time too large to be finite is not a time, nor a
/// window.
#[test]
fn unstated_a_time_too_large_to_be_finite_is_none() {
    let huge = format!("1{}", "0".repeat(305));
    assert_eq!(seconds(&format!("{huge}h")), None);
    assert_eq!(seconds(&format!("{huge}:00:00")), None);
    assert!(error(&format!("within:{huge}h")).contains("within:"));
}

/// Unstated: a window of no time is refused.
#[test]
fn unstated_a_window_of_no_time_is_refused() {
    assert!(error("within:0").contains("within:"));
}
