<!-- TOC -->
* [STRQL - STRING QUERY LANGUAGE](#strql---string-query-language)
  * [An example](#an-example)
  * [OVERVIEW OF CONCEPTS](#overview-of-concepts)
  * [STATEMENTS](#statements)
    * [DISAMBIGUATORS](#disambiguators)
  * [CAPTURES](#captures)
<!-- TOC -->

# STRQL - STRING QUERY LANGUAGE

Strql is a domain-specific language for capturing text into a structured JSON.

This project provides both the specification (this document) and a reference implementation in Rust.

## An example

```text
---
text
---
cats, dogs and birds are animals
chairs and tables are furniture
tomatoes are vegetables - not fruits!

---
query
---
TEXT = l SPLITBY NEWLINE
l = members " are " kind 0..1 (" - " comment) -> ADD item{} TO ROOT.results[]
members = member SPLITBY member_separator
member_separator = ", " OR " and "
member = WORD  -> ADD TO item.members[]
kind = WORD    -> ADD TO item
comment = LINE -> ADD TO item
```

result:

```json
{
  "results": [
    {
      "kind": "animals",
      "members": [
        "cats",
        "dogs",
        "birds"
      ]
    },
    {
      "kind": "furniture",
      "members": [
        "chairs",
        "tables"
      ]
    },
    {
      "kind": "vegetables",
      "members": [
        "tomatoes"
      ],
      "comment": "not fruits!"
    }
  ]
}
```

## OVERVIEW OF CONCEPTS

___

A Strql query is composed by a set of **statements**. The input text is successfully queried when there exists a unique way to partition
it such that every statement is satisfied. A statement has the form `<variable> = <expression>`.

After a statement it's possible to specify a **capture** by appending `-> ADD <optional/variable> TO <path>`. Every portion
of the text represented by `variable` will end up in the JSON section indicated by `path`.

## STATEMENTS

> The structure of a statement is: `<variable> = <expression>`

The **variable** can be either an identifier given by the user, or the builtin variable `TEXT`, which acts as an entry point of sorts.

During execution, `TEXT` (or rather, its associated expression) is required to match the entire input text.

___

The **expression** side of the assignment defines what structure the text must adhere to in order to match the variable.

What follows is a list of the building blocks of an expression:

| Element                         | Constraint                                                                      | Example                          |
|---------------------------------|---------------------------------------------------------------------------------|----------------------------------|
| `"<literal>"`                   | matches that raw text                                                           | name = "John"                    |
| `<variable>`                    | matches another variable                                                        | firstname = name                 |
| `<expr1> OR <expr2>`            | matches either one of the two subexpressions                                    | salutation = "hi, " OR "hello, " |
| `<expr1> <expr2>`               | matches the concatenation of those subexpressions                               | greeting = salutation name       |
| `<min>..<max> <expr>`           | matches the subexpression repeated between min and max times (included)[^1][^2] | aaas = 1..n "a"                  |
| `NEWLINE`                       | matches a newline character                                                     |                                  |
| `<expr> SPLITBY <separator>`    | shorthand for `<expr> 0..n (<separator> <expr>)`                                | lines = TEXT SPLITBY NEWLINE     |
| `(<statement>)`                 | matches the variable defined in the statement between parentheses[^3]           | yell = (aaas = 1..n "a") "h!"    |
| `LETTER`                        | matches a lowercase or uppercase letter                                         |                                  |
| `WORD`                          | shorthand for `1..N LETTER`                                                     |                                  |
| `DIGIT`                         | matches a digit                                                                 |                                  |
| `SPACE`                         | matches a whitespace character                                                  |                                  |
| `ANYCHAR`                       | matches any character                                                           |                                  |
| `ANY`                           | shorthand for  `0..N ANYCHAR`                                                   |                                  |
| `ALPHANUM`                      | shorthand for  `1..N (LETTER OR DIGIT)`                                         |                                  |
| `LINE`                          | content up to (not including) newline                                           |                                  |
| `<UPPER/LOWER/ANYCASE>``<expr>` | matches the expression with the specified case sensitivity                      |                                  |

**NOTES**: 

> Grouping an element with parentheses `()` prioritizes its execution over other elements in the expresssion.
>
> For example `1..2 ("A" OR "B")` as opposed to `(1..2 "A") OR "B"`.

> In the rest of the document we call any expression containing a repetition (`min..max`) a **quantifier**.
> This includes builtins like `WORD`, `SPLITBY` etc.

[^1]: `max` can be `n`, indicating that the repetition is unbound (equivalent to "repeat `<expression>` at least `<min>` times).

[^2]: `3..3` repeats exactly three times.

[^3]: you could also specify capture paths inside the inlined statement.


### DISAMBIGUATORS

___

Remember that the text successfully matches when it has exactly one partition that satisfies the statement set.
When there are no solutions, it's a simple matter: the rules just don't apply to the given input text.

Perhaps more subtle is the situation where the text is ambiguous with respect to the statements. Consider this example:

```text
---
text
---
a. b. c.

---
query
---
TEXT = w SPLITBY "."
w = ANY -> ADD TO ROOT.results[]
```

As you can see, there's more than one way to correctly partition the text: both `[a. b.][ c.]` and `[a.][ b. c.]` 
are valid solutions to the query. 

In this example, this happens because there are two quantifying expressions
(`SPLITBY` and `ANY`, which are both `n..m` repetitions of smaller parts) that can take an arbitrary portion of the text
and leave the rest to the other.  Indeed, the difference between `[a. b.][ c.]` and `[a.][ b. c.]` can be seen as a question
of when the `SPLITBY` rule is applied.

To resolve such ambiguities, use `LAZY` or `GREEDY` modifiers on quantifiers and `SPLITBY` expressions.

- **GREEDY**: Prefer matching more (more repetitions, more splits)
- **LAZY**: Prefer matching less (fewer repetitions, fewer splits)

```text
---
query (disambiguated)
---
TEXT = w GREEDY SPLITBY "."
w = ANY -> ADD TO ROOT.results[]
```

In this example `GREEDY SPLITBY` favors solutions where this statement is applied more times, which in this example is:

```json
{
  "results": [
    "a",
    " b",
    " c"
  ]
}
```

By contrast, replacing `GREEDY` with `LAZY` would explicitly tell the `SPLITBY` expression to match as few times as possible,
resulting in:

```json
{
  "results": [
    "a. b. c."
  ]
}
```

> If two derivations still have equal preference after applying modifiers, the parse remains ambiguous and will error.

## CAPTURES

___

> The structure of a capture is: `<variable> = <expression> -> ADD <optionally a variable> TO <capture path>`

By appending a capture to a statement, every time the latter is satisfied, which could be any amount of times between 0
(consider an optional `0..1 <expr>`) and n (in the case of quantifiers), the value represented by `variable` will be
inserted into the JSON, following the instructions given by `path`.

To be more precise, the value represented by the **variable name** differs in the following situations:

- `ADD var TO <path>` --> it represents a variable used in the statement. The held value is the text slice matching that statement variable.  
- `ADD TO <path>` --> a shorthand for the above case, where var is the left hand side of the statement.
- `ADD item{} TO <path>` --> adds an empty object to the given path, exposing `item` as a path for subsequent captures.

___

The **capture path** references where in the JSON structure to save the capture. It is structured as a series of path
segments joined by dots. In the list of segment types below, `key` references the captured variable name, and `value` its value:

| Segment type | Example                  | Explanation                                                                                     |
|--------------|--------------------------|-------------------------------------------------------------------------------------------------|
| ROOT         |                          | adds a `key`->`value` field to the very top of the JSON structure                               |
| key          | `<path>.<segment>`       | adds a `key`->`value` field to a `<segment>` section, creating it if it doesn't exist           |
| array        | `<path>.items[]`         | appends `value` to the `items` array field, creating it if it doesn't exist                     |
| named key    | `<path>.<segment>[var2]` | adds a `value2` -> `value` field to `<segment>`, where `value2` is the captured value of `var2` |