<a name=""></a>
##  (2016-09-25)


#### Performance

*   Use a single mutex for both the stack and gc ([20fb0645](https://github.com/Marwes/gluon/commit/20fb0645fd681914157a848c69b7694aee9d88af))
* **check:**
  *  Avoid traversing the entire stack when generalizing ([29352bc3](https://github.com/Marwes/gluon/commit/29352bc38f211cb6427c6107f1b178310b0db84b))
  *  Avoid recreating new App instances in unroll_app unnecessarily ([ba4db236](https://github.com/Marwes/gluon/commit/ba4db236d793bb5e23ae2463512cef191827f7c9))

#### Features

*   Use InFile to display source information for parse errors ([7026d8a3](https://github.com/Marwes/gluon/commit/7026d8a374d780e9b0f27b9910bd229e6160b28d))
*   Return `Errors<Spanned<E>>` as the parser error type ([cd838f7c](https://github.com/Marwes/gluon/commit/cd838f7c9c7656afdaacce8c5423929efa903fb7))
*   Use starts_with and ends_with from Rust instead of gluon ([5144ee29](https://github.com/Marwes/gluon/commit/5144ee295d423ca95f96a35b687906c603ea19fb))
*   Rename io.print to io.println and add io.print ([0a6b65bd](https://github.com/Marwes/gluon/commit/0a6b65bdd3e95dff737f6a846a9c2eafa1fd9581))
*   Implement unification of row polymorphic records ([df007c6e](https://github.com/Marwes/gluon/commit/df007c6e8337f582466b75e4a25c3e300a7093ee))
*   Update the rustyline dependency to 1.0.0 (#128) ([94957b61](https://github.com/Marwes/gluon/commit/94957b61ee6cd6f99ae28bb0c09ef2cf5d83cb9c))
*   Improve readability of large types by splitting them onto multiple lines ([1c296ac9](https://github.com/Marwes/gluon/commit/1c296ac9841dba57f93defc416135d2bc1a8c90d))
* **base:**  Use quick-error for instantiate::Error ([96a8c631](https://github.com/Marwes/gluon/commit/96a8c63101ea2bfd02f2351eca4fa18cb80f8ef2))
* **check:**
  *  Attempt to generate variable starting with a unique letter ([f3c2e625](https://github.com/Marwes/gluon/commit/f3c2e625dda1a5779f4915898fb9219770a7a5db))
* **instantiate:**  Unroll Data((->), [a, b]) into Function(a, b) ([09c12f01](https://github.com/Marwes/gluon/commit/09c12f01e7e822a4f5cd400cd9017ba9d873b8f5))
* **parser:**
  *  Use string slices in tokens ([e0b7d840](https://github.com/Marwes/gluon/commit/e0b7d840cdb9095bb52f39f5ab08ec5d5a68b851))
  *  Emit spans from the lexer instead of just locations ([e2a17a3a](https://github.com/Marwes/gluon/commit/e2a17a3a1e6cacf4cb9254c50bb16ae1f09aa577))
* **repl:**  Add completion to the repl ([ee4d0b60](https://github.com/Marwes/gluon/commit/ee4d0b60aa83f17e481ec96d048524b76b0b3645))
* **vm:**
  *  Implement field access of polymorphic records ([4696cedc](https://github.com/Marwes/gluon/commit/4696cedcc0a25e796361c010cddd8e8405e9d678))
  *  Allow the heap size on each thread to be limited ([f8a71f4c](https://github.com/Marwes/gluon/commit/f8a71f4cb79744c12fabb8c2edb0e199a37750c3))
  *  Return Result instead of Status in Pushable::push ([584c3590](https://github.com/Marwes/gluon/commit/584c35903f1af2856a09e5178d2cd01e21155aca))

#### Bug Fixes

*   Don't gluon panic when writing only a colon (`:`) in the repl ([7864c449](https://github.com/Marwes/gluon/commit/7864c44912561dbdd218ce28bda5465fad1f81ad))
*   Only print a Stacktrace on panics ([c059bfd3](https://github.com/Marwes/gluon/commit/c059bfd33d8a0908019fc397c19e1682f4886d6e))
*   Surround operators with parens when pretty-printing ([7ccc6f22](https://github.com/Marwes/gluon/commit/7ccc6f229f48f0077bbb90f666cad137ebfab788), closes [#60](https://github.com/Marwes/gluon/issues/60))
*   Rename windows file separators characters ('\\') to '.' as well ([207bfc9a](https://github.com/Marwes/gluon/commit/207bfc9a658cf97aca40ff5eaff8c86e36d3474b))
*   Add a space before : when pretty printing types ([a9b160c3](https://github.com/Marwes/gluon/commit/a9b160c3725584702b14f76e44bbc63487024268))
*   Print ',' as separator between each type of a record ([d72d3e1b](https://github.com/Marwes/gluon/commit/d72d3e1b7c9d4d7313a89837d0ad184ad1cfe41c))
* **check:**
  *  Fail typechecking when records use a field more than once ([7bb8f0bd](https://github.com/Marwes/gluon/commit/7bb8f0bdfc7c25de7e3bf4f19e624bbaca784ac3))
  *  Handle unification with Type::Hole ([2912727f](https://github.com/Marwes/gluon/commit/2912727f496c11680a277ce7bc2323a4abb6a6ac))
  *  Detect recursive types for which unification do not terminate ([22b3c82e](https://github.com/Marwes/gluon/commit/22b3c82ee0955ebcfec4e2367696d28629b8c7a3))
* **completion:**
  *  Give completion for local variables when pointing to whitespace ([5c59a795](https://github.com/Marwes/gluon/commit/5c59a795f8558e5f1711a033f17142b29a001451))
* **repl:**
  *  Allow `:i` to be used on primitive types ([fe458488](https://github.com/Marwes/gluon/commit/fe458488ca336df0e604d1962ab4dcef089565a6))
  *  Include the prelude when using `:t` ([bb0f1347](https://github.com/Marwes/gluon/commit/bb0f1347f327c8d1e7327db26e374bb8d759a0eb))
