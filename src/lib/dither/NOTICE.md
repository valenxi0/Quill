# dither-core

The files in this folder are the `dither-core` engine from dithercn
(https://dithercn.dev, source at https://github.com/opencoredev/dithercn),
by OpenCore, MIT licensed. They are copied unchanged from the registry item
https://dithercn.dev/r/dither-core.json. Quill uses it to draw the Dither
recording pill.

Import from `@/lib/dither/shader` and `@/lib/dither/motion`, not the index:
`index.ts` and `diffuse.ts` (image dithering, unused here) are left out of
type checking in tsconfig.json because `diffuse.ts` doesn't pass
`noUncheckedIndexedAccess`.
