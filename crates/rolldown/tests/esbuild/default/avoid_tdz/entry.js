import assert from "node:assert"

class Foo {
	static foo = new Foo
}
let foo = Foo.foo
assert(foo instanceof Foo, true)
export class Bar {}
export let bar = 123
