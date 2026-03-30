#include <stdbool.h>
#include <config.h>
#include <dwg.h>
#include <dwg_api.h>

typedef enum BridgeDwgFieldKind {
  BRIDGE_DWG_FIELD_NONE = 0,
  BRIDGE_DWG_FIELD_STRING = 1,
  BRIDGE_DWG_FIELD_HANDLE = 2,
  BRIDGE_DWG_FIELD_INTEGER = 3,
  BRIDGE_DWG_FIELD_DOUBLE = 4,
  BRIDGE_DWG_FIELD_BOOL = 5,
  BRIDGE_DWG_FIELD_POINT2D = 6,
  BRIDGE_DWG_FIELD_POINT3D = 7
} BridgeDwgFieldKind;

typedef struct BridgeDwgFieldValue {
  int kind;
  int owns_string;
  char *string_value;
  BITCODE_RLL handle_value;
  long long integer_value;
  double double_value;
  double point_x;
  double point_y;
  double point_z;
} BridgeDwgFieldValue;

Dwg_Data *bridge_dwg_data_new(void);
int bridge_dwg_data_read_file(Dwg_Data *dwg, const char *filename);
BITCODE_BL bridge_dwg_data_num_objects(const Dwg_Data *dwg);
void bridge_dwg_data_free(Dwg_Data *dwg);

const Dwg_Object *bridge_dwg_object_at(const Dwg_Data *dwg, BITCODE_BL index);
const char *bridge_dwg_object_name(const Dwg_Object *obj);
BITCODE_RLL bridge_dwg_object_handle_value(const Dwg_Object *obj);
bool bridge_dwg_object_is_entity(const Dwg_Object *obj);
BITCODE_RLL bridge_dwg_entity_owner_handle(const Dwg_Object *obj);
bool bridge_dwg_object_read_field(const Dwg_Object *obj,
                                 const char *fieldname,
                                 BridgeDwgFieldValue *out);
char *bridge_dwg_object_read_field_json(const Dwg_Object *obj,
                                        const char *fieldname);
char *bridge_dwg_object_dictionary_items_json (const Dwg_Object *obj);
char *bridge_dwg_object_dictionary_item_handles_json (const Dwg_Object *obj);
char *bridge_dwg_object_xrecord_xdata_json (const Dwg_Object *obj);
char *bridge_dwg_object_evaluation_graph_nodes_json (const Dwg_Object *obj);
char *bridge_dwg_object_evaluation_graph_edges_json (const Dwg_Object *obj);
void bridge_dwg_string_free (char *value);
void bridge_dwg_field_value_free(BridgeDwgFieldValue *value);
