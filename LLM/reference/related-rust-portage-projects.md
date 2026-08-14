Luca è conosciuto anche come lu_zero developer Gentoo

Ho avuto un'interessante discussione con lui dove ho scoperto che ci sono altri due progetti che stanno facendo qualcosa di simile a quanto stiamo facendo noi con questo porting.

Un estratto delle info interessanti:

I due progetti di riscrittura Portage in rust:
- https://github.com/pkgcraft
- https://github.com/lu-zero/portage-cli

hanno licenza rispettivamente  "AS-IS" e "MIT"

portage-cli contiene al suo interno vari tool di gentoo:
- emerge
- crossdev
- portage-utils
- gentoolkit

il tutto in tanti tanti piccoli crate pronti al riuso

Alcune documentazioni dei suoi crate:
- https://docs.rs/gentoo-core/latest/gentoo_core/
- https://docs.rs/portage-atom/latest/portage_atom/

inoltre ha moltissimi test:

```
cargo nextest list | wc -l
Finished test profile [unoptimized + debuginfo] target(s) in 0.32s
1886
```

