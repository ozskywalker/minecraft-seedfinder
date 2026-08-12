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

/* getBiomeAt(g, scale, x, z) for the Overworld at sea level (y=63). Returns a Java
 * biome id, or -1 (none) on failure.
 *
 * NOTE: getBiomeAt's signature is (g, scale, x, y, z) — the y argument is passed as
 * 63 (sea level) so we get the SURFACE biome, not the deep-cave layer. A previous
 * version of this bridge swapped y and z (passing z as y and hardcoding z=63), which
 * returned deep_dark (the cave biome) at essentially every surface coordinate and
 * made the Bedrock↔Java biome agreement look catastrophically low. Surface biome
 * checks must sample at surface height (see cubiomes' own examples, which use y=63). */
int sf_biome_at(const Generator *g, int scale, int x, int z)
{
    return getBiomeAt(g, scale, x, 63, z);
}

/* Diagnostic variant: sample at an explicit y. */
int sf_biome_at_y(const Generator *g, int scale, int x, int z, int y)
{
    return getBiomeAt(g, scale, x, y, z);
}

/* The cubiomes version constant for the newest supported MC release (MC_1_21). */
int sf_mc_latest(void)
{
    return MC_1_21;
}
