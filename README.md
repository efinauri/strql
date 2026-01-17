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
l = members " are " kind 0..1 comment 
                    -> ADD item{} TO ROOT.results[]
members = member SPLITBY member_separator
member_separator = ", " OR " and "
member = WORD       -> ADD TO item.members[]
kind = WORD         -> ADD TO item
comment = "-" LINE  -> ADD TO item
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

The query on the text is a set of **statements**. The text successfully parses when there exists a unique way to partition
it such that every statement is satisfied. A statement has the form `<variable> = <expression>`.

After a statement it's possible to specify a **capture** by appending `-> ADD <optionally a variable> TO <capture path>`. This is an instruction
on how to build the JSON output.

## STATEMENTS

___

Again, the structure of a statement is `<variable> = <expression>`.

The **variable** can be any identifier, or the builtin variable `TEXT`, which acts as an entry point of sorts.
During execution, `TEXT`, or rather, its associated expression, is required to match the entire input text.

___

**Expressions** are a combination of elements that dictate the structure that any text that matches to the variable has to adhere to.
Said elements are:

| Element                              | Constraint                                                                   |
|--------------------------------------|------------------------------------------------------------------------------|
| `<literal>`                          | matches that raw text                                                        |
| `<variable>`                         | matches another variable                                                     |
| `<expr1> OR <expr2>`                 | matches either one of the two expressions                                    |
| `<min>..<max> <expr>`                | matches the expression repeated between min and max times (included)[^1][^2] |
| `<expr> SPLITBY <separator>`         | shorthand for `<expr> 0..n (<separator> <expr>)`                             | 
| `LETTER`                             | matches a lowercase or uppercase letter                                      |
| `WORD`                               | equivalent to `1..N LETTER`                                                  |
| `DIGIT`                              | matches a digit                                                              |
| `SPACE`                              | matches a whitespace character                                               |
| `NEWLINE`                            | matches a newline character                                                  |
| `ANYCHAR`                            | matches any character                                                        |
| `ANY`                                | equivalent to `0..N ANYCHAR`                                                 |
| `ALPHANUM`                           | equivalent to `1..N (LETTER OR DIGIT)`                                       |
| `LINE`                               | content up to (not including) newline                                        |
| `UPPER`, `LOWER`, `ANYCASE` `<expr>` | matches the expression with the specified case sensitivity                   |

**NOTE**: grouping an element with parentheses `()` prioritizes its execution over other elements in the expresssion.
  For example `1..2 ("A" OR "B")` as opposed to `(1..2 "A") OR "B"`.

[^1]: `min` and `max` can also be variables. A repetition with a lower or higher bound indicated by a variable is unbound
in that direction, i.e. `1..N` means "repeated at least once" and `N..2` means "repeated at most twice."

[^2]: `3..3` repeats exactly three times.


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
and leave the rest to the other.  Indeed, the difference between `[a. b.][ c.]` and `[a.][ b. c.]` could be imputed to
when the `SPLITBY` rule is applied.

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

In this example:
- `GREEDY SPLITBY` prefers more splits (more `.` separators consumed)

If two derivations still have equal preference after applying modifiers, the parse remains ambiguous and will error.

## CAPTURES

___

In this case as well let us examine the syntax of a capture: `<variable> = <expression> -> ADD <optionally a variable> TO <capture path>`.
This makes the capture happen every time that statement is satisfied, which could be any amount of times between 0
(consider an optional `0..1 <expr>`) and n (in the case of quantifiers).

A **variable name** tells the capture what should be put into the specified path. This can either be an actual piece of text
if the variable references something that is already matching a statement, or an empty named object. 
If absent, `<variable>` from the left hand side of the statement is used.

For these three cases the syntax is as follows:

- `ADD var TO <path>`
- `ADD item{} TO <path>`
- `ADD TO <path>`

___

The **capture path** references where in the JSON structure to save the capture. It is structured as a series of path
segments joined by dots. In the list of segment types below, `key` references the captured variable name, and `value` its value:

| Segment type | Example                  | Explanation                                                                                     |
|--------------|--------------------------|-------------------------------------------------------------------------------------------------|
| ROOT         |                          | adds a `key`->`value` field to the very top of the JSON structure                               |
| key          | `<path>.<segment>`       | adds a `key`->`value` field to a `<segment>` section, creating it if it doesn't exist           |
| array        | `<path>.items[]`         | appends `value` to the `items` array field, creating it if it doesn't exist                     |
| named key    | `<path>.<segment>[var2]` | adds a `value2` -> `value` field to `<segment>`, where `value2` is the captured value of `var2` |