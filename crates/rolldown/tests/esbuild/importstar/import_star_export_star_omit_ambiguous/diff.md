## /out.js
### esbuild
```js
// common.js
var common_exports = {};
__export(common_exports, {
  x: () => x,
  z: () => z
});

// foo.js
var x = 1;

// bar.js
var z = 4;

// entry.js
console.log(common_exports);
```
### rolldown
```js
import { default as assert } from "node:assert";


//#region foo.js
const x = 1;

//#endregion
//#region bar.js
const z = 4;

//#endregion
//#region common.js
var common_ns = {};
__export(common_ns, {
	x: () => x,
	z: () => z
});

//#endregion
//#region entry.js
assert.deepEqual(common_ns, {
	x: 1,
	z: 4
});

//#endregion

```
### diff
```diff
===================================================================
--- esbuild	/out.js
+++ rolldown	entry_js.mjs
@@ -1,8 +1,11 @@
-var common_exports = {};
-__export(common_exports, {
+const x = 1;
+const z = 4;
+var common_ns = {};
+__export(common_ns, {
     x: () => x,
     z: () => z
 });
-var x = 1;
-var z = 4;
-console.log(common_exports);
\ No newline at end of file
+assert.deepEqual(common_ns, {
+    x: 1,
+    z: 4
+});
\ No newline at end of file

```