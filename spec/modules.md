# Module loading (current implementation)

A `use` path is resolved relative to the importing file. Files are identified by their filesystem-canonicalized paths. Dependencies are loaded depth-first in source order, before the importing file; a completed file is loaded only once. Importing a file already on the active dependency path is an error. Multiple paths reaching a completed file are allowed.

Import depth must not rely on recursive host-language calls. Loading errors prevent compilation; a failed module is never treated as successfully loaded.
