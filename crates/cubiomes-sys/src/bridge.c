/*
 * cubiomes-sys bridge: a small, stable C surface so Rust never needs to know the
 * (union-heavy) layout of cubiomes' Generator struct.
 *
 * cubiomes is MIT-licensed (see cubiomes/LICENSE) and vendored here per PLAN §2.7
 * ("Link + vendor (biomes)").
 */
#include "generator.h"
#include <stdlib.h>

/* Allocate a zeroed Generator on the C side; Rust holds only an opaque pointer. */
Generator *sf_generator_new(void)
{
    return (Generator *)calloc(1, sizeof(Generator));
}

void sf_generator_free(Generator *g)
{
    free(g);
}

/* setupGenerator(g, mc, flags=0). */
void sf_setup(Generator *g, int mc)
{
    setupGenerator(g, mc, 0);
}

/* applySeed(g, dim, seed). */
void sf_apply_seed(Generator *g, int dim, uint64_t seed)
{
    applySeed(g, dim, seed);
}

/* getBiomeAt(g, scale, x, z) for the Overworld (y=0). Returns a Java biome id, or
 * -1 (none) on failure. */
int sf_biome_at(const Generator *g, int scale, int x, int z)
{
    return getBiomeAt(g, scale, x, z, 0);
}

/* The cubiomes version constant for the newest supported MC release (MC_1_21). */
int sf_mc_latest(void)
{
    return MC_1_21;
}
