// The Cryst bundle is a classic script that reads React from the global scope.
// This module must be evaluated before the bundle, so it lives on its own and
// is imported first by ./index.ts.
import * as React from "react";

Object.assign(globalThis, { React });
