#include "../include/iron_vaak_v0.h"

int main(void) {
    IronVaakAbiInfoV0 info = {0};
    IronVaakHostLayoutEntryV0 layout = {0};
    IronVaakSnapshotRecordV0 snapshot = {0};
    IronVaakPatchRecordV0 patch = {0};
    return (int)(info.struct_size + layout.slot_index + snapshot.property_id + patch.property_id);
}
