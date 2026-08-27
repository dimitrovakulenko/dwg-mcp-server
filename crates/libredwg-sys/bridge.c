#include "bridge.h"

#include <bits.h>
#include <ctype.h>
#include <math.h>
#include <stdint.h>
#include <stdlib.h>
#include <stdio.h>
#include <stdarg.h>
#include <string.h>

typedef struct BridgeDwgData
{
  Dwg_Data dwg;
  Dwg_Object_Ref **sorted_object_refs;
  BITCODE_BL num_sorted_object_refs;
} BridgeDwgData;

static int
bridge_compare_object_ref_ptrs (const void *left, const void *right)
{
  const uintptr_t left_value
      = (uintptr_t)*(Dwg_Object_Ref *const *)left;
  const uintptr_t right_value
      = (uintptr_t)*(Dwg_Object_Ref *const *)right;
  return (left_value > right_value) - (left_value < right_value);
}

static void
bridge_index_object_refs (Dwg_Data *dwg)
{
  BridgeDwgData *data = (BridgeDwgData *)dwg;

  free (data->sorted_object_refs);
  data->sorted_object_refs = NULL;
  data->num_sorted_object_refs = 0;
  if (!dwg->object_ref || !dwg->num_object_refs
      || dwg->num_object_refs > SIZE_MAX / sizeof (*data->sorted_object_refs))
    return;

  data->sorted_object_refs
      = (Dwg_Object_Ref **)malloc (dwg->num_object_refs
                                  * sizeof (*data->sorted_object_refs));
  if (!data->sorted_object_refs)
    return;
  memcpy (data->sorted_object_refs, dwg->object_ref,
          dwg->num_object_refs * sizeof (*data->sorted_object_refs));
  data->num_sorted_object_refs = dwg->num_object_refs;
  qsort (data->sorted_object_refs, data->num_sorted_object_refs,
         sizeof (*data->sorted_object_refs), bridge_compare_object_ref_ptrs);
}

Dwg_Data *
bridge_dwg_data_new(void)
{
  BridgeDwgData *data = (BridgeDwgData *)calloc (1, sizeof (BridgeDwgData));
  return data ? &data->dwg : NULL;
}

int
bridge_dwg_data_read_file(Dwg_Data *dwg, const char *filename)
{
  int status;
  size_t length;

  if (!dwg || !filename)
    return DWG_ERR_IOERROR;
  length = strlen (filename);
  if (length >= 4
      && tolower ((unsigned char)filename[length - 4]) == '.'
      && tolower ((unsigned char)filename[length - 3]) == 'd'
      && tolower ((unsigned char)filename[length - 2]) == 'x'
      && tolower ((unsigned char)filename[length - 1]) == 'f')
    status = dxf_read_file(filename, dwg);
  else
    status = dwg_read_file(filename, dwg);
  if (status < DWG_ERR_CRITICAL)
    bridge_index_object_refs (dwg);
  return status;
}

BITCODE_BL
bridge_dwg_data_num_objects(const Dwg_Data *dwg)
{
  return dwg ? dwg->num_objects : 0;
}

void
bridge_dwg_data_free(Dwg_Data *dwg)
{
  BridgeDwgData *data;

  if (!dwg)
    return;
  data = (BridgeDwgData *)dwg;
  free (data->sorted_object_refs);
  dwg_free(dwg);
  free(data);
}

const Dwg_Object *
bridge_dwg_object_at(const Dwg_Data *dwg, BITCODE_BL index)
{
  if (!dwg || !dwg->object || index < 0 || index >= dwg->num_objects)
    return NULL;
  return &dwg->object[index];
}

const char *
bridge_dwg_object_name(const Dwg_Object *obj)
{
  return obj ? obj->name : NULL;
}

BITCODE_RLL
bridge_dwg_object_handle_value(const Dwg_Object *obj)
{
  return obj ? obj->handle.value : 0;
}

static const void *bridge_dwg_object_specific_ptr(const Dwg_Object *obj);
static bool bridge_dwg_object_has_decoded_specific_data (const Dwg_Object *obj);

typedef struct BridgeJsonBuffer
{
  char *data;
  size_t len;
  size_t cap;
} BridgeJsonBuffer;

static bool bridge_json_buffer_reserve (BridgeJsonBuffer *buffer, size_t extra);
static bool bridge_json_buffer_append_mem (BridgeJsonBuffer *buffer,
                                          const char *text, size_t length);
static bool bridge_json_buffer_append_cstr (BridgeJsonBuffer *buffer,
                                           const char *text);
static bool bridge_json_buffer_append_char (BridgeJsonBuffer *buffer, char ch);
static bool bridge_json_buffer_appendf (BridgeJsonBuffer *buffer,
                                       const char *format, ...);
static bool bridge_json_buffer_append_json_string (BridgeJsonBuffer *buffer,
                                                  const char *text);
static const Dwg_Object_DICTIONARY *
bridge_dwg_dictionary_ptr (const Dwg_Object *obj);
static const Dwg_Object_DICTIONARYWDFLT *
bridge_dwg_dictionarywdflt_ptr (const Dwg_Object *obj);
static const Dwg_Object_XRECORD *
bridge_dwg_xrecord_ptr (const Dwg_Object *obj);
static const Dwg_Entity_HATCH *
bridge_dwg_hatch_ptr (const Dwg_Object *obj);
static bool bridge_json_append_hex_string (BridgeJsonBuffer *buffer,
                                          const unsigned char *bytes,
                                          size_t length);
static bool bridge_json_append_resbuf_value (BridgeJsonBuffer *buffer,
                                            const Dwg_Resbuf *rbuf);
static bool bridge_json_append_point2d_values (BridgeJsonBuffer *buffer,
                                               double x, double y);
static bool bridge_json_append_point2rd (BridgeJsonBuffer *buffer,
                                         BITCODE_2RD point);
static bool bridge_json_append_hatch_boundary_handles (
    BridgeJsonBuffer *buffer, const Dwg_Object *obj, BITCODE_H *handles,
    BITCODE_BL count);
static bool bridge_json_append_hatch_segment (BridgeJsonBuffer *buffer,
                                              const Dwg_HATCH_PathSeg *segment);
static bool bridge_json_append_hatch_segments (BridgeJsonBuffer *buffer,
                                               const Dwg_HATCH_Path *path);
static bool bridge_json_append_hatch_polyline_points (
    BridgeJsonBuffer *buffer, const Dwg_HATCH_Path *path);
static bool bridge_json_append_hatch_polyline_bulges (
    BridgeJsonBuffer *buffer, const Dwg_HATCH_Path *path);
static bool bridge_json_append_hatch_contour (BridgeJsonBuffer *buffer,
                                              const Dwg_Object *obj,
                                              const Dwg_HATCH_Path *path,
                                              BITCODE_BL index);
static uint32_t bridge_read_u32_le (const unsigned char *bytes);
static double bridge_read_double_le (const unsigned char *bytes);
static bool bridge_type_is_point_array (const char *type);
static bool bridge_try_read_count_field (const Dwg_Object *obj,
                                        const char *count_field,
                                        BITCODE_BL *out_count);
static bool bridge_read_array_count (const Dwg_Object *obj,
                                    const char *fieldname,
                                    BITCODE_BL *out_count);

bool
bridge_dwg_object_is_entity(const Dwg_Object *obj)
{
  return obj && obj->supertype == DWG_SUPERTYPE_ENTITY;
}

static bool
bridge_json_buffer_reserve (BridgeJsonBuffer *buffer, size_t extra)
{
  size_t required;
  char *next;

  if (!buffer)
    return false;

  required = buffer->len + extra + 1;
  if (required <= buffer->cap)
    return true;

  if (buffer->cap == 0)
    buffer->cap = 256;
  while (buffer->cap < required)
    buffer->cap *= 2;

  next = (char *)realloc (buffer->data, buffer->cap);
  if (!next)
    return false;

  buffer->data = next;
  return true;
}

static bool
bridge_json_buffer_append_mem (BridgeJsonBuffer *buffer, const char *text,
                              size_t length)
{
  if (!buffer || (!text && length != 0)
      || !bridge_json_buffer_reserve (buffer, length))
    return false;

  if (length != 0)
    memcpy (buffer->data + buffer->len, text, length);
  buffer->len += length;
  buffer->data[buffer->len] = '\0';
  return true;
}

static bool
bridge_json_buffer_append_cstr (BridgeJsonBuffer *buffer, const char *text)
{
  if (!text)
    text = "";
  return bridge_json_buffer_append_mem (buffer, text, strlen (text));
}

static bool
bridge_json_buffer_append_char (BridgeJsonBuffer *buffer, char ch)
{
  return bridge_json_buffer_append_mem (buffer, &ch, 1);
}

static bool
bridge_json_buffer_appendf (BridgeJsonBuffer *buffer, const char *format, ...)
{
  va_list args;
  va_list copy;
  int written;

  if (!buffer || !format)
    return false;

  va_start (args, format);
  va_copy (copy, args);
  written = vsnprintf (NULL, 0, format, copy);
  va_end (copy);
  if (written < 0 || !bridge_json_buffer_reserve (buffer, (size_t)written))
    {
      va_end (args);
      return false;
    }

  vsnprintf (buffer->data + buffer->len, buffer->cap - buffer->len, format,
             args);
  va_end (args);
  buffer->len += (size_t)written;
  return true;
}

static bool
bridge_json_buffer_append_json_string (BridgeJsonBuffer *buffer, const char *text)
{
  const unsigned char *cursor;

  if (!bridge_json_buffer_append_char (buffer, '"'))
    return false;

  if (!text)
    text = "";

  cursor = (const unsigned char *)text;
  while (*cursor)
    {
      switch (*cursor)
        {
        case '\\':
          if (!bridge_json_buffer_append_cstr (buffer, "\\\\"))
            return false;
          break;
        case '"':
          if (!bridge_json_buffer_append_cstr (buffer, "\\\""))
            return false;
          break;
        case '\b':
          if (!bridge_json_buffer_append_cstr (buffer, "\\b"))
            return false;
          break;
        case '\f':
          if (!bridge_json_buffer_append_cstr (buffer, "\\f"))
            return false;
          break;
        case '\n':
          if (!bridge_json_buffer_append_cstr (buffer, "\\n"))
            return false;
          break;
        case '\r':
          if (!bridge_json_buffer_append_cstr (buffer, "\\r"))
            return false;
          break;
        case '\t':
          if (!bridge_json_buffer_append_cstr (buffer, "\\t"))
            return false;
          break;
        default:
          if (*cursor < 0x20)
            {
              if (!bridge_json_buffer_appendf (buffer, "\\u%04X",
                                              (unsigned)*cursor))
                return false;
            }
          else if (!bridge_json_buffer_append_char (buffer, (char)*cursor))
            return false;
        }
      cursor++;
    }

  return bridge_json_buffer_append_char (buffer, '"');
}

BITCODE_RLL
bridge_dwg_entity_owner_handle(const Dwg_Object *obj)
{
  const void *specific;
  Dwg_Object_BLOCK_HEADER *owner;

  if (!obj || obj->supertype != DWG_SUPERTYPE_ENTITY)
    return 0;

  specific = bridge_dwg_object_specific_ptr(obj);
  if (!specific)
    return 0;

  owner = dwg_entity_owner(specific);
  if (!owner)
    return 0;

  return dwg_obj_generic_handlevalue(owner);
}

static void *
bridge_dwg_object_common_ptr(const Dwg_Object *obj)
{
  return (void *)bridge_dwg_object_specific_ptr(obj);
}

static const void *
bridge_dwg_object_specific_ptr(const Dwg_Object *obj)
{
  if (!obj)
    return NULL;
  if (obj->supertype == DWG_SUPERTYPE_ENTITY && obj->tio.entity)
    return obj->tio.entity->tio.UNKNOWN_ENT;
  if (obj->supertype == DWG_SUPERTYPE_OBJECT && obj->tio.object)
    return obj->tio.object->tio.UNKNOWN_OBJ;
  return NULL;
}

static bool
bridge_dwg_object_has_decoded_specific_data (const Dwg_Object *obj)
{
  if (!obj)
    return false;
  return obj->fixedtype != DWG_TYPE_UNKNOWN_ENT
         && obj->fixedtype != DWG_TYPE_UNKNOWN_OBJ;
}

static const Dwg_Object_DICTIONARY *
bridge_dwg_dictionary_ptr (const Dwg_Object *obj)
{
  if (!obj || obj->supertype != DWG_SUPERTYPE_OBJECT || !obj->name
      || strcmp (obj->name, "DICTIONARY") != 0)
    return NULL;
  return (const Dwg_Object_DICTIONARY *)bridge_dwg_object_specific_ptr (obj);
}

static const Dwg_Object_DICTIONARYWDFLT *
bridge_dwg_dictionarywdflt_ptr (const Dwg_Object *obj)
{
  if (!obj || obj->supertype != DWG_SUPERTYPE_OBJECT || !obj->name
      || strcmp (obj->name, "DICTIONARYWDFLT") != 0)
    return NULL;
  return (const Dwg_Object_DICTIONARYWDFLT *)bridge_dwg_object_specific_ptr (obj);
}

static const Dwg_Object_XRECORD *
bridge_dwg_xrecord_ptr (const Dwg_Object *obj)
{
  if (!obj || obj->supertype != DWG_SUPERTYPE_OBJECT || !obj->name
      || strcmp (obj->name, "XRECORD") != 0)
    return NULL;
  return (const Dwg_Object_XRECORD *)bridge_dwg_object_specific_ptr (obj);
}

static const Dwg_Object_EVALUATION_GRAPH *
bridge_dwg_evaluation_graph_ptr (const Dwg_Object *obj)
{
  if (!obj || obj->supertype != DWG_SUPERTYPE_OBJECT || !obj->name
      || strcmp (obj->name, "EVALUATION_GRAPH") != 0)
    return NULL;
  return (const Dwg_Object_EVALUATION_GRAPH *)bridge_dwg_object_specific_ptr (obj);
}

static const Dwg_Entity_HATCH *
bridge_dwg_hatch_ptr (const Dwg_Object *obj)
{
  if (!obj || obj->supertype != DWG_SUPERTYPE_ENTITY || !obj->name
      || strcmp (obj->name, "HATCH") != 0)
    return NULL;
  return (const Dwg_Entity_HATCH *)bridge_dwg_object_specific_ptr (obj);
}

static const Dwg_Entity_LWPOLYLINE *
bridge_dwg_lwpolyline_ptr (const Dwg_Object *obj)
{
  if (!obj || obj->supertype != DWG_SUPERTYPE_ENTITY || !obj->name
      || strcmp (obj->name, "LWPOLYLINE") != 0)
    return NULL;
  return (const Dwg_Entity_LWPOLYLINE *)bridge_dwg_object_specific_ptr (obj);
}

static const Dwg_Entity_SPLINE *
bridge_dwg_spline_ptr (const Dwg_Object *obj)
{
  if (!obj || obj->supertype != DWG_SUPERTYPE_ENTITY || !obj->name
      || strcmp (obj->name, "SPLINE") != 0)
    return NULL;
  return (const Dwg_Entity_SPLINE *)bridge_dwg_object_specific_ptr (obj);
}

static const Dwg_Entity_WIPEOUT *
bridge_dwg_wipeout_ptr (const Dwg_Object *obj)
{
  if (!obj || obj->supertype != DWG_SUPERTYPE_ENTITY)
    return NULL;
  return (const Dwg_Entity_WIPEOUT *)bridge_dwg_object_specific_ptr (obj);
}

static const Dwg_DYNAPI_field *
bridge_dwg_common_field(const Dwg_Object *obj, const char *fieldname)
{
  if (!obj || !fieldname)
    return NULL;
  if (obj->supertype == DWG_SUPERTYPE_ENTITY)
    return dwg_dynapi_common_entity_field(fieldname);
  if (obj->supertype == DWG_SUPERTYPE_OBJECT)
    return dwg_dynapi_common_object_field(fieldname);
  return NULL;
}

static bool
bridge_read_string_field(const Dwg_Object *obj, const char *fieldname,
                        bool is_common, BridgeDwgFieldValue *out,
                        Dwg_DYNAPI_field *fp)
{
  int isnew = 0;
  char *text = NULL;
  bool ok;
  void *common = bridge_dwg_object_common_ptr(obj);

  if (is_common)
    {
      if (!common)
        return false;
      ok = dwg_dynapi_common_utf8text(common, fieldname, &text, &isnew, fp);
    }
  else
    {
      const void *specific = bridge_dwg_object_specific_ptr(obj);
      if (!specific)
        return false;
      ok = dwg_dynapi_entity_utf8text((void *)specific, obj->name, fieldname,
                                      &text, &isnew, fp);
    }

  if (!ok || !text)
    return false;

  out->kind = BRIDGE_DWG_FIELD_STRING;
  out->string_value = text;
  out->owns_string = isnew;
  return true;
}

static bool
bridge_read_header_string_field(const Dwg_Data *dwg, const char *fieldname,
                               BridgeDwgFieldValue *out, Dwg_DYNAPI_field *fp)
{
  int isnew = 0;
  char *text = NULL;

  if (!dwg || !dwg_dynapi_header_utf8text (dwg, fieldname, &text, &isnew, fp)
      || !text)
    return false;

  out->kind = BRIDGE_DWG_FIELD_STRING;
  out->string_value = text;
  out->owns_string = isnew;
  return true;
}

static bool
bridge_read_raw_field(const Dwg_Object *obj, const char *fieldname, bool is_common,
                     void *buffer, Dwg_DYNAPI_field *fp)
{
  if (is_common)
    {
      void *common = bridge_dwg_object_common_ptr(obj);
      if (!common)
        return false;
      return dwg_dynapi_common_value(common, fieldname, buffer, fp);
    }

  const void *specific = bridge_dwg_object_specific_ptr(obj);
  if (!specific)
    return false;

  return dwg_dynapi_entity_value((void *)specific, obj->name, fieldname, buffer,
                                 fp);
}

static bool
bridge_read_header_raw_field(const Dwg_Data *dwg, const char *fieldname,
                            void *buffer, Dwg_DYNAPI_field *fp)
{
  if (!dwg)
    return false;

  return dwg_dynapi_header_value (dwg, fieldname, buffer, fp);
}

static bool
bridge_type_matches(const char *type, const char *candidate)
{
  return type && strcmp(type, candidate) == 0;
}

static bool
bridge_handle_ref_is_known(const Dwg_Data *dwg, BITCODE_H ref)
{
  const BridgeDwgData *data = (const BridgeDwgData *)dwg;
  BITCODE_BL index;

  if (!dwg || !dwg->object_ref || !ref)
    return false;

  if (data->sorted_object_refs)
    return bsearch (&ref, data->sorted_object_refs,
                    data->num_sorted_object_refs,
                    sizeof (*data->sorted_object_refs),
                    bridge_compare_object_ref_ptrs)
           != NULL;

  for (index = 0; index < dwg->num_object_refs; index++)
    {
      if (dwg->object_ref[index] == ref)
        return true;
    }

  return false;
}

static bool
bridge_try_get_handle_value(const Dwg_Object *obj, BITCODE_H ref,
                            BITCODE_RLL *out_value)
{
  if (!out_value)
    return false;

  *out_value = 0;
  if (!ref)
    return true;

  if (!obj || !bridge_handle_ref_is_known(obj->parent, ref))
    return false;

  *out_value = ref->absolute_ref;
  return true;
}

static bool
bridge_try_get_header_handle_value(const Dwg_Data *dwg, BITCODE_H ref,
                                  BITCODE_RLL *out_value)
{
  if (!out_value)
    return false;

  *out_value = 0;
  if (!ref)
    return true;

  if (!dwg || !bridge_handle_ref_is_known (dwg, ref))
    return false;

  *out_value = ref->absolute_ref;
  return true;
}

static bool
bridge_try_convert_time_field (const char *type, const BITCODE_TIMEBLL *value,
                              BridgeDwgFieldValue *out)
{
  if (!value || !out
      || (!bridge_type_matches (type, "TIMEBLL")
          && !bridge_type_matches (type, "TIMERLL")))
    return false;

  out->kind = BRIDGE_DWG_FIELD_TIME;
  out->time_days = value->days;
  out->time_ms = value->ms;
  out->time_value = value->value;
  return true;
}

bool
bridge_dwg_header_read_field(const Dwg_Data *dwg, const char *fieldname,
                             BridgeDwgFieldValue *out)
{
  Dwg_DYNAPI_field fp;
  const Dwg_DYNAPI_field *field;

  if (!dwg || !fieldname || !out)
    return false;
  memset (out, 0, sizeof (*out));

  field = dwg_dynapi_header_field (fieldname);
  if (!field)
    return false;
  memcpy (&fp, field, sizeof (fp));

  if (fp.is_string)
    {
      if (bridge_type_matches (fp.type, "TF") || bridge_type_matches (fp.type, "TFv"))
        return false;

      return bridge_read_header_string_field (dwg, fieldname, out, &fp);
    }

  if (strchr (fp.type, '*'))
    return false;

  if (strchr (fp.type, 'H'))
    {
      BITCODE_H ref = NULL;
      BITCODE_RLL handle_value = 0;
      if (fp.size != sizeof (ref))
        return false;
      if (!bridge_read_header_raw_field (dwg, fieldname, &ref, &fp))
        return false;
      if (!bridge_try_get_header_handle_value (dwg, ref, &handle_value))
        return false;
      out->kind = BRIDGE_DWG_FIELD_HANDLE;
      out->handle_value = handle_value;
      return true;
    }

  if (bridge_type_matches (fp.type, "2RD") || bridge_type_matches (fp.type, "2BD")
      || bridge_type_matches (fp.type, "2DD")
      || bridge_type_matches (fp.type, "2DPOINT"))
    {
      dwg_point_2d point;
      if (fp.size != sizeof (point))
        return false;
      if (!bridge_read_header_raw_field (dwg, fieldname, &point, &fp))
        return false;
      out->kind = BRIDGE_DWG_FIELD_POINT2D;
      out->point_x = point.x;
      out->point_y = point.y;
      return true;
    }

  if (bridge_type_matches (fp.type, "CMC") || bridge_type_matches (fp.type, "CMTC")
      || bridge_type_matches (fp.type, "ENC"))
    {
      Dwg_Color color;
      if (fp.size != sizeof (color))
        return false;
      if (!bridge_read_header_raw_field (dwg, fieldname, &color, &fp))
        return false;
      out->kind = BRIDGE_DWG_FIELD_INTEGER;
      out->integer_value = color.index;
      return true;
    }

  if (bridge_type_matches (fp.type, "3BD") || bridge_type_matches (fp.type, "3BD_1")
      || bridge_type_matches (fp.type, "3RD") || bridge_type_matches (fp.type, "3DD")
      || bridge_type_matches (fp.type, "BE")
      || bridge_type_matches (fp.type, "3DPOINT"))
    {
      dwg_point_3d point;
      if (fp.size != sizeof (point))
        return false;
      if (!bridge_read_header_raw_field (dwg, fieldname, &point, &fp))
        return false;
      out->kind = BRIDGE_DWG_FIELD_POINT3D;
      out->point_x = point.x;
      out->point_y = point.y;
      out->point_z = point.z;
      return true;
    }

  if (bridge_type_matches (fp.type, "BD") || bridge_type_matches (fp.type, "RD")
      || bridge_type_matches (fp.type, "BT"))
    {
      double value = 0.0;
      if (fp.size != sizeof (value))
        return false;
      if (!bridge_read_header_raw_field (dwg, fieldname, &value, &fp))
        return false;
      out->kind = BRIDGE_DWG_FIELD_DOUBLE;
      out->double_value = value;
      return true;
    }

  if (bridge_type_matches (fp.type, "B") || bridge_type_matches (fp.type, "BB"))
    {
      BITCODE_B value = 0;
      if (fp.size != sizeof (value))
        return false;
      if (!bridge_read_header_raw_field (dwg, fieldname, &value, &fp))
        return false;
      out->kind = BRIDGE_DWG_FIELD_BOOL;
      out->integer_value = value != 0;
      return true;
    }

  if (bridge_type_matches (fp.type, "RC"))
    {
      BITCODE_RC value = 0;
      if (fp.size != sizeof (value))
        return false;
      if (!bridge_read_header_raw_field (dwg, fieldname, &value, &fp))
        return false;
      out->kind = BRIDGE_DWG_FIELD_INTEGER;
      out->integer_value = value;
      return true;
    }

  if (bridge_type_matches (fp.type, "RS") || bridge_type_matches (fp.type, "BS")
      || bridge_type_matches (fp.type, "BSd") || bridge_type_matches (fp.type, "RSd"))
    {
      BITCODE_BS value = 0;
      if (fp.size != sizeof (value))
        return false;
      if (!bridge_read_header_raw_field (dwg, fieldname, &value, &fp))
        return false;
      out->kind = BRIDGE_DWG_FIELD_INTEGER;
      out->integer_value = value;
      return true;
    }

  if (bridge_type_matches (fp.type, "RL") || bridge_type_matches (fp.type, "BL"))
    {
      BITCODE_BL value = 0;
      if (fp.size != sizeof (value))
        return false;
      if (!bridge_read_header_raw_field (dwg, fieldname, &value, &fp))
        return false;
      out->kind = BRIDGE_DWG_FIELD_INTEGER;
      out->integer_value = value;
      return true;
    }

  if (bridge_type_matches (fp.type, "BLL") || bridge_type_matches (fp.type, "RLL"))
    {
      BITCODE_RLL value = 0;
      if (fp.size != sizeof (value))
        return false;
      if (!bridge_read_header_raw_field (dwg, fieldname, &value, &fp))
        return false;
      out->kind = BRIDGE_DWG_FIELD_INTEGER;
      out->integer_value = (long long)value;
      return true;
    }

  if (bridge_type_matches (fp.type, "BLd"))
    {
      BITCODE_BLd value = 0;
      if (fp.size != sizeof (value))
        return false;
      if (!bridge_read_header_raw_field (dwg, fieldname, &value, &fp))
        return false;
      out->kind = BRIDGE_DWG_FIELD_INTEGER;
      out->integer_value = (long long)value;
      return true;
    }

  if (bridge_type_matches (fp.type, "TIMEBLL")
      || bridge_type_matches (fp.type, "TIMERLL"))
    {
      BITCODE_TIMEBLL value;
      if (fp.size != sizeof (value))
        return false;
      if (!bridge_read_header_raw_field (dwg, fieldname, &value, &fp))
        return false;
      return bridge_try_convert_time_field (fp.type, &value, out);
    }

  return false;
}

bool
bridge_dwg_object_read_field(const Dwg_Object *obj, const char *fieldname,
                            BridgeDwgFieldValue *out)
{
  Dwg_DYNAPI_field fp;
  const Dwg_DYNAPI_field *field;
  bool is_common;

  if (!obj || !fieldname || !out)
    return false;
  memset(out, 0, sizeof(*out));

  field = bridge_dwg_common_field(obj, fieldname);
  is_common = field != NULL;
  if (!field)
    {
      if (!bridge_dwg_object_has_decoded_specific_data (obj))
        return false;
      field = dwg_dynapi_entity_field(obj->name, fieldname);
      if (!field)
        return false;
    }
  memcpy(&fp, field, sizeof(fp));

  if (fp.is_string)
    {
      /* TF/TFv are binary payloads (e.g. previews), not guaranteed C strings. */
      if (bridge_type_matches(fp.type, "TF") || bridge_type_matches(fp.type, "TFv"))
        return false;

      return bridge_read_string_field(obj, fieldname, is_common, out, &fp);
    }

  if (strchr(fp.type, '*'))
    return false;

  if (strchr(fp.type, 'H'))
    {
      BITCODE_H ref = NULL;
      BITCODE_RLL handle_value = 0;
      if (fp.size != sizeof(ref))
        return false;
      if (!bridge_read_raw_field(obj, fieldname, is_common, &ref, &fp))
        return false;
      if (!bridge_try_get_handle_value(obj, ref, &handle_value))
        return false;
      out->kind = BRIDGE_DWG_FIELD_HANDLE;
      out->handle_value = handle_value;
      return true;
    }

  if (bridge_type_matches(fp.type, "2RD") || bridge_type_matches(fp.type, "2BD")
      || bridge_type_matches(fp.type, "2DD")
      || bridge_type_matches(fp.type, "2DPOINT"))
    {
      dwg_point_2d point;
      if (fp.size != sizeof(point))
        return false;
      if (!bridge_read_raw_field(obj, fieldname, is_common, &point, &fp))
        return false;
      out->kind = BRIDGE_DWG_FIELD_POINT2D;
      out->point_x = point.x;
      out->point_y = point.y;
      return true;
    }

  if (bridge_type_matches(fp.type, "CMC") || bridge_type_matches(fp.type, "CMTC")
      || bridge_type_matches(fp.type, "ENC"))
    {
      Dwg_Color color;
      if (fp.size != sizeof(color))
        return false;
      if (!bridge_read_raw_field(obj, fieldname, is_common, &color, &fp))
        return false;
      out->kind = BRIDGE_DWG_FIELD_INTEGER;
      out->integer_value = color.index;
      return true;
    }

  if (bridge_type_matches(fp.type, "3BD") || bridge_type_matches(fp.type, "3BD_1")
      || bridge_type_matches(fp.type, "3RD") || bridge_type_matches(fp.type, "3DD")
      || bridge_type_matches(fp.type, "BE")
      || bridge_type_matches(fp.type, "3DPOINT"))
    {
      dwg_point_3d point;
      if (fp.size != sizeof(point))
        return false;
      if (!bridge_read_raw_field(obj, fieldname, is_common, &point, &fp))
        return false;
      out->kind = BRIDGE_DWG_FIELD_POINT3D;
      out->point_x = point.x;
      out->point_y = point.y;
      out->point_z = point.z;
      return true;
    }

  if (bridge_type_matches(fp.type, "BD") || bridge_type_matches(fp.type, "RD")
      || bridge_type_matches(fp.type, "BT"))
    {
      double value = 0.0;
      if (fp.size != sizeof(value))
        return false;
      if (!bridge_read_raw_field(obj, fieldname, is_common, &value, &fp))
        return false;
      out->kind = BRIDGE_DWG_FIELD_DOUBLE;
      out->double_value = value;
      return true;
    }

  if (bridge_type_matches(fp.type, "B") || bridge_type_matches(fp.type, "BB"))
    {
      BITCODE_B value = 0;
      if (fp.size != sizeof(value))
        return false;
      if (!bridge_read_raw_field(obj, fieldname, is_common, &value, &fp))
        return false;
      out->kind = BRIDGE_DWG_FIELD_BOOL;
      out->integer_value = value != 0;
      return true;
    }

  if (bridge_type_matches(fp.type, "RC"))
    {
      BITCODE_RC value = 0;
      if (fp.size != sizeof(value))
        return false;
      if (!bridge_read_raw_field(obj, fieldname, is_common, &value, &fp))
        return false;
      out->kind = BRIDGE_DWG_FIELD_INTEGER;
      out->integer_value = value;
      return true;
    }

  if (bridge_type_matches(fp.type, "RS") || bridge_type_matches(fp.type, "BS")
      || bridge_type_matches(fp.type, "BSd") || bridge_type_matches(fp.type, "RSd"))
    {
      BITCODE_BS value = 0;
      if (fp.size != sizeof(value))
        return false;
      if (!bridge_read_raw_field(obj, fieldname, is_common, &value, &fp))
        return false;
      out->kind = BRIDGE_DWG_FIELD_INTEGER;
      out->integer_value = value;
      return true;
    }

  if (bridge_type_matches(fp.type, "RL") || bridge_type_matches(fp.type, "BL"))
    {
      BITCODE_BL value = 0;
      if (fp.size != sizeof(value))
        return false;
      if (!bridge_read_raw_field(obj, fieldname, is_common, &value, &fp))
        return false;
      out->kind = BRIDGE_DWG_FIELD_INTEGER;
      out->integer_value = value;
      return true;
    }

  if (bridge_type_matches(fp.type, "BLL") || bridge_type_matches(fp.type, "RLL"))
    {
      BITCODE_RLL value = 0;
      if (fp.size != sizeof(value))
        return false;
      if (!bridge_read_raw_field(obj, fieldname, is_common, &value, &fp))
        return false;
      out->kind = BRIDGE_DWG_FIELD_INTEGER;
      out->integer_value = (long long)value;
      return true;
    }

  if (bridge_type_matches(fp.type, "BLd"))
    {
      BITCODE_BLd value = 0;
      if (fp.size != sizeof(value))
        return false;
      if (!bridge_read_raw_field(obj, fieldname, is_common, &value, &fp))
        return false;
      out->kind = BRIDGE_DWG_FIELD_INTEGER;
      out->integer_value = (long long)value;
      return true;
    }

  if (bridge_type_matches (fp.type, "TIMEBLL")
      || bridge_type_matches (fp.type, "TIMERLL"))
    {
      BITCODE_TIMEBLL value;
      if (fp.size != sizeof (value))
        return false;
      if (!bridge_read_raw_field (obj, fieldname, is_common, &value, &fp))
        return false;
      return bridge_try_convert_time_field (fp.type, &value, out);
    }

  return false;
}

static uint32_t
bridge_read_u32_le (const unsigned char *bytes)
{
  return ((uint32_t)bytes[0]) | ((uint32_t)bytes[1] << 8)
         | ((uint32_t)bytes[2] << 16) | ((uint32_t)bytes[3] << 24);
}

static double
bridge_read_double_le (const unsigned char *bytes)
{
  uint64_t raw = ((uint64_t)bytes[0]) | ((uint64_t)bytes[1] << 8)
                 | ((uint64_t)bytes[2] << 16) | ((uint64_t)bytes[3] << 24)
                 | ((uint64_t)bytes[4] << 32) | ((uint64_t)bytes[5] << 40)
                 | ((uint64_t)bytes[6] << 48) | ((uint64_t)bytes[7] << 56);
  double value;
  memcpy (&value, &raw, sizeof (value));
  return value;
}

static bool
bridge_type_is_point_array (const char *type)
{
  return bridge_type_matches (type, "2RD*") || bridge_type_matches (type, "2BD*")
         || bridge_type_matches (type, "2DPOINT*")
         || bridge_type_matches (type, "3RD*")
         || bridge_type_matches (type, "3BD*")
         || bridge_type_matches (type, "3DPOINT*")
         || bridge_type_matches (type, "BE*");
}

static bool
bridge_try_read_count_field (const Dwg_Object *obj, const char *count_field,
                            BITCODE_BL *out_count)
{
  BridgeDwgFieldValue value;
  long long candidate = 0;
  bool ok = false;

  if (!obj || !count_field || !out_count)
    return false;

  if (!bridge_dwg_object_read_field (obj, count_field, &value))
    return false;

  switch (value.kind)
    {
    case BRIDGE_DWG_FIELD_INTEGER:
      candidate = value.integer_value;
      ok = true;
      break;
    case BRIDGE_DWG_FIELD_DOUBLE:
      candidate = (long long)value.double_value;
      ok = true;
      break;
    case BRIDGE_DWG_FIELD_BOOL:
      candidate = value.integer_value ? 1 : 0;
      ok = true;
      break;
    default:
      break;
    }
  bridge_dwg_field_value_free (&value);

  if (!ok || candidate < 0)
    return false;

  *out_count = (BITCODE_BL)candidate;
  return true;
}

static bool
bridge_read_array_count (const Dwg_Object *obj, const char *fieldname,
                        BITCODE_BL *out_count)
{
  char candidate[128];
  size_t length;

  if (!obj || !fieldname || !out_count)
    return false;

  if (snprintf (candidate, sizeof (candidate), "num_%s", fieldname) > 0
      && bridge_try_read_count_field (obj, candidate, out_count))
    return true;

  length = strlen (fieldname);
  if (length > 1 && fieldname[length - 1] == 's'
      && snprintf (candidate, sizeof (candidate), "num_%.*s",
                   (int)(length - 1), fieldname) > 0
      && bridge_try_read_count_field (obj, candidate, out_count))
    return true;

  if (strcmp (fieldname, "vertex") == 0
      && (bridge_try_read_count_field (obj, "num_vertex", out_count)
          || bridge_try_read_count_field (obj, "num_owned", out_count)))
    return true;

  if (strcmp (fieldname, "verts") == 0
      && bridge_try_read_count_field (obj, "num_verts", out_count))
    return true;

  return false;
}

char *
bridge_dwg_object_read_field_json (const Dwg_Object *obj, const char *fieldname)
{
  Dwg_DYNAPI_field fp;
  const Dwg_DYNAPI_field *field;
  bool is_common;
  BITCODE_BL count = 0;
  void *values = NULL;
  BridgeJsonBuffer buffer = { 0 };

  if (!obj || !fieldname)
    return NULL;

  field = bridge_dwg_common_field (obj, fieldname);
  is_common = field != NULL;
  if (!field)
    {
      if (!bridge_dwg_object_has_decoded_specific_data (obj))
        return NULL;
      field = dwg_dynapi_entity_field (obj->name, fieldname);
      if (!field)
        return NULL;
    }
  memcpy (&fp, field, sizeof (fp));

  if (!strchr (fp.type, '*') || !bridge_type_is_point_array (fp.type))
    return NULL;

  if (!bridge_read_array_count (obj, fieldname, &count))
    return NULL;

  if (count == 0)
    return strdup ("[]");

  if (fp.size != sizeof (values))
    return NULL;
  if (!bridge_read_raw_field (obj, fieldname, is_common, &values, &fp) || !values)
    return NULL;

  if (!bridge_json_buffer_append_char (&buffer, '['))
    goto failed;

  for (BITCODE_BL index = 0; index < count; index++)
    {
      double x = 0.0;
      double y = 0.0;
      double z = 0.0;
      bool is_3d = false;

      if (bridge_type_matches (fp.type, "2RD*"))
        {
          BITCODE_2RD *items = (BITCODE_2RD *)values;
          x = items[index].x;
          y = items[index].y;
        }
      else if (bridge_type_matches (fp.type, "2BD*")
               || bridge_type_matches (fp.type, "2DPOINT*"))
        {
          BITCODE_2BD *items = (BITCODE_2BD *)values;
          x = items[index].x;
          y = items[index].y;
        }
      else if (bridge_type_matches (fp.type, "3RD*"))
        {
          BITCODE_3RD *items = (BITCODE_3RD *)values;
          x = items[index].x;
          y = items[index].y;
          z = items[index].z;
          is_3d = true;
        }
      else if (bridge_type_matches (fp.type, "3BD*")
               || bridge_type_matches (fp.type, "3DPOINT*")
               || bridge_type_matches (fp.type, "BE*"))
        {
          BITCODE_3BD *items = (BITCODE_3BD *)values;
          x = items[index].x;
          y = items[index].y;
          z = items[index].z;
          is_3d = true;
        }
      else
        goto failed;

      if (index > 0 && !bridge_json_buffer_append_char (&buffer, ','))
        goto failed;
      if (is_3d)
        {
          if (!bridge_json_buffer_appendf (&buffer, "[%.17g,%.17g,%.17g]", x, y, z))
            goto failed;
        }
      else if (!bridge_json_buffer_appendf (&buffer, "[%.17g,%.17g]", x, y))
        goto failed;
    }

  if (!bridge_json_buffer_append_char (&buffer, ']'))
    goto failed;
  return buffer.data;

failed:
  free (buffer.data);
  return NULL;
}

typedef struct BridgeProxyTableText
{
  double x;
  double y;
  double z;
  char *text;
} BridgeProxyTableText;

typedef struct BridgeProxyTableLine
{
  double x1;
  double y1;
  double z1;
  double x2;
  double y2;
  double z2;
} BridgeProxyTableLine;

static int
bridge_compare_doubles (const void *left, const void *right)
{
  double a = *(const double *)left;
  double b = *(const double *)right;
  return (a > b) - (a < b);
}

static bool
bridge_append_double (double **items, size_t *count, size_t *capacity,
                     double value)
{
  double *next;

  if (*count == *capacity)
    {
      size_t next_capacity = *capacity ? *capacity * 2 : 8;
      next = (double *)realloc (*items, next_capacity * sizeof (**items));
      if (!next)
        return false;
      *items = next;
      *capacity = next_capacity;
    }

  (*items)[(*count)++] = value;
  return true;
}

static bool
bridge_append_proxy_text (BridgeProxyTableText **items, size_t *count,
                         size_t *capacity, BridgeProxyTableText value)
{
  BridgeProxyTableText *next;

  if (*count == *capacity)
    {
      size_t next_capacity = *capacity ? *capacity * 2 : 8;
      next = (BridgeProxyTableText *)realloc (*items,
                                             next_capacity * sizeof (**items));
      if (!next)
        return false;
      *items = next;
      *capacity = next_capacity;
    }

  (*items)[(*count)++] = value;
  return true;
}

static bool
bridge_append_proxy_line (BridgeProxyTableLine **items, size_t *count,
                         size_t *capacity, BridgeProxyTableLine value)
{
  BridgeProxyTableLine *next;

  if (*count == *capacity)
    {
      size_t next_capacity = *capacity ? *capacity * 2 : 8;
      next = (BridgeProxyTableLine *)realloc (*items,
                                             next_capacity * sizeof (**items));
      if (!next)
        return false;
      *items = next;
      *capacity = next_capacity;
    }

  (*items)[(*count)++] = value;
  return true;
}

static char *
bridge_proxy_utf16le_string (const unsigned char *bytes, size_t length)
{
  size_t chars = 0;
  char *text;

  while ((chars + 1) * 2 <= length)
    {
      uint16_t ch = (uint16_t)bytes[chars * 2]
                    | ((uint16_t)bytes[chars * 2 + 1] << 8);
      if (ch == 0)
        break;
      chars++;
    }

  if (chars == 0)
    return NULL;

  text = (char *)calloc (chars + 1, 1);
  if (!text)
    return NULL;

  for (size_t index = 0; index < chars; index++)
    {
      uint16_t ch = (uint16_t)bytes[index * 2]
                    | ((uint16_t)bytes[index * 2 + 1] << 8);
      text[index] = ch >= 0x20 && ch <= 0x7E ? (char)ch : '?';
    }

  return text;
}

static size_t
bridge_unique_sorted_doubles (double *items, size_t count)
{
  size_t output = 0;
  const double epsilon = 1e-9;

  if (!items || count == 0)
    return 0;

  qsort (items, count, sizeof (*items), bridge_compare_doubles);
  for (size_t index = 0; index < count; index++)
    {
      if (output == 0 || fabs (items[index] - items[output - 1]) > epsilon)
        items[output++] = items[index];
    }
  return output;
}

static int
bridge_find_proxy_table_column (const double *x_values, size_t count, double x)
{
  const double epsilon = 1e-9;

  if (!x_values || count < 2)
    return -1;

  for (size_t index = 0; index + 1 < count; index++)
    {
      if (x >= x_values[index] - epsilon && x <= x_values[index + 1] + epsilon)
        return (int)index;
    }
  return -1;
}

static int
bridge_find_proxy_table_row (const double *y_values, size_t count, double y)
{
  const double epsilon = 1e-9;

  if (!y_values || count < 2)
    return -1;

  for (size_t index = 0; index + 1 < count; index++)
    {
      if (y >= y_values[index] - epsilon && y <= y_values[index + 1] + epsilon)
        return (int)(count - 2 - index);
    }
  return -1;
}

static const BridgeProxyTableText *
bridge_proxy_table_cell_text (const BridgeProxyTableText *texts,
                             const int *text_rows, const int *text_cols,
                             size_t text_count, size_t row, size_t col)
{
  for (size_t index = 0; index < text_count; index++)
    {
      if (text_rows[index] == (int)row && text_cols[index] == (int)col)
        return &texts[index];
    }
  return NULL;
}

char *
bridge_dwg_object_proxy_table_json (const Dwg_Object *obj)
{
  Dwg_Object_Entity *entity;
  const unsigned char *preview;
  size_t preview_size;
  uint32_t total_size;
  uint32_t record_count;
  size_t offset = 8;
  BridgeProxyTableText *texts = NULL;
  size_t text_count = 0;
  size_t text_capacity = 0;
  BridgeProxyTableLine *lines = NULL;
  size_t line_count = 0;
  size_t line_capacity = 0;
  double *x_values = NULL;
  double *y_values = NULL;
  size_t x_count = 0;
  size_t y_count = 0;
  size_t x_capacity = 0;
  size_t y_capacity = 0;
  int *text_rows = NULL;
  int *text_cols = NULL;
  BridgeJsonBuffer buffer = { 0 };

  if (!obj || !obj->name || strcmp (obj->name, "TABLE") != 0
      || obj->supertype != DWG_SUPERTYPE_ENTITY || !obj->tio.entity
      || obj->fixedtype != DWG_TYPE_UNKNOWN_ENT)
    return NULL;

  entity = obj->tio.entity;
  preview = (const unsigned char *)entity->preview;
  preview_size = (size_t)entity->preview_size;
  if (!preview || preview_size < 12 || preview_size > 1024 * 1024)
    return NULL;

  total_size = bridge_read_u32_le (preview);
  record_count = bridge_read_u32_le (preview + 4);
  if (total_size != preview_size || record_count == 0 || record_count > 10000)
    return NULL;

  for (uint32_t record_index = 0;
       record_index < record_count && offset + 8 <= preview_size;
       record_index++)
    {
      uint32_t record_size = bridge_read_u32_le (preview + offset);
      uint32_t opcode = bridge_read_u32_le (preview + offset + 4);
      const unsigned char *payload = preview + offset + 8;
      size_t payload_size;

      if (record_size < 8 || offset + record_size > preview_size)
        goto failed;

      payload_size = (size_t)record_size - 8;
      if (opcode == 0x26 && payload_size >= 76)
        {
          char *text = bridge_proxy_utf16le_string (payload + 72,
                                                   payload_size - 72);
          if (text)
            {
              BridgeProxyTableText item;
              item.x = bridge_read_double_le (payload);
              item.y = bridge_read_double_le (payload + 8);
              item.z = bridge_read_double_le (payload + 16);
              item.text = text;
              if (!bridge_append_proxy_text (&texts, &text_count,
                                             &text_capacity, item))
                {
                  free (text);
                  goto failed;
                }
            }
        }
      else if (opcode == 0x20 && payload_size >= 52
               && bridge_read_u32_le (payload) == 2)
        {
          BridgeProxyTableLine line;
          line.x1 = bridge_read_double_le (payload + 4);
          line.y1 = bridge_read_double_le (payload + 12);
          line.z1 = bridge_read_double_le (payload + 20);
          line.x2 = bridge_read_double_le (payload + 28);
          line.y2 = bridge_read_double_le (payload + 36);
          line.z2 = bridge_read_double_le (payload + 44);
          if (!bridge_append_proxy_line (&lines, &line_count, &line_capacity,
                                         line))
            goto failed;
        }

      offset += record_size;
    }

  if (text_count == 0 || line_count == 0)
    goto failed;

  for (size_t index = 0; index < line_count; index++)
    {
      const double epsilon = 1e-9;
      BridgeProxyTableLine *line = &lines[index];
      if (fabs (line->y1 - line->y2) <= epsilon
          && fabs (line->x1 - line->x2) > epsilon)
        {
          if (!bridge_append_double (&y_values, &y_count, &y_capacity, line->y1))
            goto failed;
        }
      else if (fabs (line->x1 - line->x2) <= epsilon
               && fabs (line->y1 - line->y2) > epsilon)
        {
          if (!bridge_append_double (&x_values, &x_count, &x_capacity, line->x1))
            goto failed;
        }
    }

  x_count = bridge_unique_sorted_doubles (x_values, x_count);
  y_count = bridge_unique_sorted_doubles (y_values, y_count);
  if (x_count < 2 || y_count < 2)
    goto failed;

  text_rows = (int *)calloc (text_count, sizeof (*text_rows));
  text_cols = (int *)calloc (text_count, sizeof (*text_cols));
  if (!text_rows || !text_cols)
    goto failed;

  for (size_t index = 0; index < text_count; index++)
    {
      text_cols[index] = bridge_find_proxy_table_column (x_values, x_count,
                                                        texts[index].x);
      text_rows[index] = bridge_find_proxy_table_row (y_values, y_count,
                                                     texts[index].y);
    }

  if (!bridge_json_buffer_appendf (
          &buffer,
          "{\"table_extraction_source\":\"proxy_preview\",\"num_rows\":%zu,\"num_cols\":%zu,\"row_heights\":[",
          y_count - 1, x_count - 1))
    goto failed;

  for (size_t row = y_count - 1; row > 0; row--)
    {
      if (row != y_count - 1 && !bridge_json_buffer_append_char (&buffer, ','))
        goto failed;
      if (!bridge_json_buffer_appendf (&buffer, "%.17g",
                                      y_values[row] - y_values[row - 1]))
        goto failed;
    }

  if (!bridge_json_buffer_append_cstr (&buffer, "],\"col_widths\":["))
    goto failed;
  for (size_t col = 0; col + 1 < x_count; col++)
    {
      if (col > 0 && !bridge_json_buffer_append_char (&buffer, ','))
        goto failed;
      if (!bridge_json_buffer_appendf (&buffer, "%.17g",
                                      x_values[col + 1] - x_values[col]))
        goto failed;
    }

  if (!bridge_json_buffer_append_cstr (&buffer, "],\"cell_texts\":["))
    goto failed;
  for (size_t row = 0; row + 1 < y_count; row++)
    {
      if (row > 0 && !bridge_json_buffer_append_char (&buffer, ','))
        goto failed;
      if (!bridge_json_buffer_append_char (&buffer, '['))
        goto failed;
      for (size_t col = 0; col + 1 < x_count; col++)
        {
          const BridgeProxyTableText *text
              = bridge_proxy_table_cell_text (texts, text_rows, text_cols,
                                             text_count, row, col);
          if (col > 0 && !bridge_json_buffer_append_char (&buffer, ','))
            goto failed;
          if (text)
            {
              if (!bridge_json_buffer_append_json_string (&buffer, text->text))
                goto failed;
            }
          else if (!bridge_json_buffer_append_cstr (&buffer, "null"))
            goto failed;
        }
      if (!bridge_json_buffer_append_char (&buffer, ']'))
        goto failed;
    }

  if (!bridge_json_buffer_append_cstr (&buffer, "],\"cells\":["))
    goto failed;
  for (size_t index = 0, emitted = 0; index < text_count; index++)
    {
      if (text_rows[index] < 0 || text_cols[index] < 0)
        continue;
      if (emitted++ > 0 && !bridge_json_buffer_append_char (&buffer, ','))
        goto failed;
      if (!bridge_json_buffer_appendf (
              &buffer,
              "{\"row\":%d,\"column\":%d,\"text\":",
              text_rows[index], text_cols[index])
          || !bridge_json_buffer_append_json_string (&buffer, texts[index].text)
          || !bridge_json_buffer_appendf (
                 &buffer,
                 ",\"position\":[%.17g,%.17g,%.17g]}",
                 texts[index].x, texts[index].y, texts[index].z))
        goto failed;
    }

  if (!bridge_json_buffer_append_cstr (&buffer, "]}"))
    goto failed;

  for (size_t index = 0; index < text_count; index++)
    free (texts[index].text);
  free (texts);
  free (lines);
  free (x_values);
  free (y_values);
  free (text_rows);
  free (text_cols);
  return buffer.data;

failed:
  for (size_t index = 0; index < text_count; index++)
    free (texts[index].text);
  free (texts);
  free (lines);
  free (x_values);
  free (y_values);
  free (text_rows);
  free (text_cols);
  free (buffer.data);
  return NULL;
}

char *
bridge_dwg_object_dictionary_items_json (const Dwg_Object *obj)
{
  const Dwg_Object_DICTIONARY *dictionary = bridge_dwg_dictionary_ptr (obj);
  const Dwg_Object_DICTIONARYWDFLT *dictionarywdflt
      = bridge_dwg_dictionarywdflt_ptr (obj);
  BITCODE_BL numitems = 0;
  BITCODE_T *texts = NULL;
  BITCODE_H *itemhandles = NULL;
  BridgeJsonBuffer buffer = { 0 };

  if (dictionary)
    {
      numitems = dictionary->numitems;
      texts = dictionary->texts;
      itemhandles = dictionary->itemhandles;
    }
  else if (dictionarywdflt)
    {
      numitems = dictionarywdflt->numitems;
      texts = dictionarywdflt->texts;
      itemhandles = dictionarywdflt->itemhandles;
    }
  else
    return NULL;

  if (!bridge_json_buffer_append_char (&buffer, '{'))
    goto failed;

  for (BITCODE_BL index = 0; index < numitems; index++)
    {
      const char *text = texts ? texts[index] : NULL;
      const BITCODE_H ref = itemhandles ? itemhandles[index] : NULL;

      if (!text)
        continue;

      if (buffer.len > 1 && !bridge_json_buffer_append_char (&buffer, ','))
        goto failed;
      if (!bridge_json_buffer_append_json_string (&buffer, text)
          || !bridge_json_buffer_append_char (&buffer, ':')
          || !bridge_json_buffer_appendf (
                 &buffer, "\"%llX\"",
                 (unsigned long long)(ref ? ref->absolute_ref : 0)))
        goto failed;
    }

  if (!bridge_json_buffer_append_char (&buffer, '}'))
    goto failed;
  return buffer.data;

failed:
  free (buffer.data);
  return NULL;
}

char *
bridge_dwg_object_dictionary_item_handles_json (const Dwg_Object *obj)
{
  const Dwg_Object_DICTIONARY *dictionary = bridge_dwg_dictionary_ptr (obj);
  const Dwg_Object_DICTIONARYWDFLT *dictionarywdflt
      = bridge_dwg_dictionarywdflt_ptr (obj);
  BITCODE_BL numitems = 0;
  BITCODE_H *itemhandles = NULL;
  BridgeJsonBuffer buffer = { 0 };

  if (dictionary)
    {
      numitems = dictionary->numitems;
      itemhandles = dictionary->itemhandles;
    }
  else if (dictionarywdflt)
    {
      numitems = dictionarywdflt->numitems;
      itemhandles = dictionarywdflt->itemhandles;
    }
  else
    return NULL;

  if (!bridge_json_buffer_append_char (&buffer, '['))
    goto failed;

  for (BITCODE_BL index = 0; index < numitems; index++)
    {
      const BITCODE_H ref = itemhandles ? itemhandles[index] : NULL;

      if (index > 0 && !bridge_json_buffer_append_char (&buffer, ','))
        goto failed;
      if (!bridge_json_buffer_appendf (
              &buffer, "\"%llX\"",
              (unsigned long long)(ref ? ref->absolute_ref : 0)))
        goto failed;
    }

  if (!bridge_json_buffer_append_char (&buffer, ']'))
    goto failed;
  return buffer.data;

failed:
  free (buffer.data);
  return NULL;
}

static bool
bridge_json_append_hex_string (BridgeJsonBuffer *buffer,
                              const unsigned char *bytes, size_t length)
{
  static const char hex[] = "0123456789ABCDEF";

  if (!bridge_json_buffer_append_char (buffer, '"'))
    return false;

  for (size_t index = 0; index < length; index++)
    {
      char pair[2];
      pair[0] = hex[(bytes[index] >> 4) & 0xF];
      pair[1] = hex[bytes[index] & 0xF];
      if (!bridge_json_buffer_append_mem (buffer, pair, sizeof (pair)))
        return false;
    }

  return bridge_json_buffer_append_char (buffer, '"');
}

static bool
bridge_json_append_resbuf_value (BridgeJsonBuffer *buffer, const Dwg_Resbuf *rbuf)
{
  enum RESBUF_VALUE_TYPE type;

  if (!buffer || !rbuf)
    return false;

  type = dwg_resbuf_value_type (rbuf->type);
  switch (type)
    {
    case DWG_VT_STRING:
      if (!rbuf->value.str.is_tu)
        return bridge_json_buffer_append_json_string (buffer,
                                                     rbuf->value.str.u.data);
      else
        {
          char *utf8 = bit_convert_TU ((BITCODE_TU)rbuf->value.str.u.wdata);
          bool ok = bridge_json_buffer_append_json_string (buffer, utf8);
          free (utf8);
          return ok;
        }
    case DWG_VT_BINARY:
      return bridge_json_append_hex_string (
          buffer, (const unsigned char *)rbuf->value.str.u.data,
          (size_t)rbuf->value.str.size);
    case DWG_VT_REAL:
      return bridge_json_buffer_appendf (buffer, "%.17g", rbuf->value.dbl);
    case DWG_VT_BOOL:
    case DWG_VT_INT8:
      return bridge_json_buffer_appendf (buffer, "%d", (int)rbuf->value.i8);
    case DWG_VT_INT16:
      return bridge_json_buffer_appendf (buffer, "%d", (int)rbuf->value.i16);
    case DWG_VT_INT32:
      return bridge_json_buffer_appendf (buffer, "%d", (int)rbuf->value.i32);
    case DWG_VT_INT64:
      return bridge_json_buffer_appendf (buffer, "%lld",
                                        (long long)rbuf->value.i64);
    case DWG_VT_POINT3D:
      return bridge_json_buffer_appendf (buffer, "[%.17g,%.17g,%.17g]",
                                        rbuf->value.pt[0], rbuf->value.pt[1],
                                        rbuf->value.pt[2]);
    case DWG_VT_HANDLE:
    case DWG_VT_OBJECTID:
      return bridge_json_buffer_appendf (buffer, "%llu",
                                        (unsigned long long)rbuf->value.absref);
    case DWG_VT_INVALID:
    default:
      return bridge_json_buffer_append_cstr (buffer, "null");
    }
}

static bool
bridge_json_append_point2d_values (BridgeJsonBuffer *buffer, double x, double y)
{
  return bridge_json_buffer_appendf (buffer, "[%.17g,%.17g]", x, y);
}

static bool
bridge_json_append_point2rd (BridgeJsonBuffer *buffer, BITCODE_2RD point)
{
  return bridge_json_append_point2d_values (buffer, point.x, point.y);
}

static bool
bridge_json_append_hatch_boundary_handles (BridgeJsonBuffer *buffer,
                                          const Dwg_Object *obj,
                                          BITCODE_H *handles,
                                          BITCODE_BL count)
{
  if (!bridge_json_buffer_append_char (buffer, '['))
    return false;

  for (BITCODE_BL index = 0; index < count; index++)
    {
      BITCODE_RLL handle_value = 0;

      if (index > 0 && !bridge_json_buffer_append_char (buffer, ','))
        return false;
      if (handles)
        bridge_try_get_handle_value (obj, handles[index], &handle_value);
      if (!bridge_json_buffer_appendf (buffer, "\"%llX\"",
                                      (unsigned long long)handle_value))
        return false;
    }

  return bridge_json_buffer_append_char (buffer, ']');
}

static bool
bridge_json_append_hatch_segment (BridgeJsonBuffer *buffer,
                                 const Dwg_HATCH_PathSeg *segment)
{
  double start_x, start_y, end_x, end_y;

  if (!buffer || !segment || !bridge_json_buffer_append_char (buffer, '{'))
    return false;

  switch (segment->curve_type)
    {
    case 1:
      if (!bridge_json_buffer_append_cstr (buffer, "\"type\":\"line\",\"points\":[")
          || !bridge_json_append_point2rd (buffer, segment->first_endpoint)
          || !bridge_json_buffer_append_char (buffer, ',')
          || !bridge_json_append_point2rd (buffer, segment->second_endpoint)
          || !bridge_json_buffer_append_char (buffer, ']'))
        return false;
      break;
    case 2:
      start_x = segment->center.x + segment->radius * cos (segment->start_angle);
      start_y = segment->center.y + segment->radius * sin (segment->start_angle);
      end_x = segment->center.x + segment->radius * cos (segment->end_angle);
      end_y = segment->center.y + segment->radius * sin (segment->end_angle);
      if (!bridge_json_buffer_append_cstr (
              buffer,
              "\"type\":\"circularArc\",\"center\":")
          || !bridge_json_append_point2rd (buffer, segment->center)
          || !bridge_json_buffer_appendf (
                 buffer,
                 ",\"radius\":%.17g,\"startAngle\":%.17g,\"endAngle\":%.17g,\"isCcw\":%s,\"startPoint\":",
                 segment->radius, segment->start_angle, segment->end_angle,
                 segment->is_ccw ? "true" : "false")
          || !bridge_json_append_point2d_values (buffer, start_x, start_y)
          || !bridge_json_buffer_append_cstr (buffer, ",\"endPoint\":")
          || !bridge_json_append_point2d_values (buffer, end_x, end_y))
        return false;
      break;
    case 3:
      {
        double major_x = segment->endpoint.x;
        double major_y = segment->endpoint.y;
        double minor_x = -major_y * segment->minor_major_ratio;
        double minor_y = major_x * segment->minor_major_ratio;

        start_x = segment->center.x + major_x * cos (segment->start_angle)
                  + minor_x * sin (segment->start_angle);
        start_y = segment->center.y + major_y * cos (segment->start_angle)
                  + minor_y * sin (segment->start_angle);
        end_x = segment->center.x + major_x * cos (segment->end_angle)
                + minor_x * sin (segment->end_angle);
        end_y = segment->center.y + major_y * cos (segment->end_angle)
                + minor_y * sin (segment->end_angle);

        if (!bridge_json_buffer_append_cstr (
                buffer,
                "\"type\":\"ellipticalArc\",\"center\":")
            || !bridge_json_append_point2rd (buffer, segment->center)
            || !bridge_json_buffer_append_cstr (buffer, ",\"majorAxisVector\":")
            || !bridge_json_append_point2rd (buffer, segment->endpoint)
            || !bridge_json_buffer_appendf (
                   buffer,
                   ",\"minorMajorRatio\":%.17g,\"startAngle\":%.17g,\"endAngle\":%.17g,\"isCcw\":%s,\"startPoint\":",
                   segment->minor_major_ratio, segment->start_angle,
                   segment->end_angle, segment->is_ccw ? "true" : "false")
            || !bridge_json_append_point2d_values (buffer, start_x, start_y)
            || !bridge_json_buffer_append_cstr (buffer, ",\"endPoint\":")
            || !bridge_json_append_point2d_values (buffer, end_x, end_y))
          return false;
      }
      break;
    case 4:
      if ((segment->num_knots > 0 && !segment->knots)
          || (segment->num_control_points > 0 && !segment->control_points)
          || (segment->num_fitpts > 0 && !segment->fitpts))
        return false;
      if (!bridge_json_buffer_appendf (
              buffer,
              "\"type\":\"spline\",\"degree\":%lld,\"isRational\":%s,\"isPeriodic\":%s,\"knots\":[",
              (long long)segment->degree,
              segment->is_rational ? "true" : "false",
              segment->is_periodic ? "true" : "false"))
        return false;
      for (BITCODE_BL index = 0; index < segment->num_knots; index++)
        {
          if (index > 0 && !bridge_json_buffer_append_char (buffer, ','))
            return false;
          if (!bridge_json_buffer_appendf (buffer, "%.17g", segment->knots[index]))
            return false;
        }
      if (!bridge_json_buffer_append_cstr (buffer, "],\"controlPoints\":["))
        return false;
      for (BITCODE_BL index = 0; index < segment->num_control_points; index++)
        {
          const Dwg_HATCH_ControlPoint *control = &segment->control_points[index];

          if (index > 0 && !bridge_json_buffer_append_char (buffer, ','))
            return false;
          if (!bridge_json_buffer_append_cstr (buffer, "{\"point\":")
              || !bridge_json_append_point2rd (buffer, control->point))
            return false;
          if (segment->is_rational
              && !bridge_json_buffer_appendf (buffer, ",\"weight\":%.17g",
                                             control->weight))
            return false;
          if (!bridge_json_buffer_append_char (buffer, '}'))
            return false;
        }
      if (!bridge_json_buffer_append_char (buffer, ']'))
        return false;
      if (segment->num_fitpts > 0)
        {
          if (!bridge_json_buffer_append_cstr (buffer, ",\"fitPoints\":["))
            return false;
          for (BITCODE_BL index = 0; index < segment->num_fitpts; index++)
            {
              if (index > 0 && !bridge_json_buffer_append_char (buffer, ','))
                return false;
              if (!bridge_json_append_point2rd (buffer, segment->fitpts[index]))
                return false;
            }
          if (!bridge_json_buffer_append_char (buffer, ']'))
            return false;
        }
      if (!bridge_json_buffer_append_cstr (buffer, ",\"startTangent\":")
          || !bridge_json_append_point2rd (buffer, segment->start_tangent)
          || !bridge_json_buffer_append_cstr (buffer, ",\"endTangent\":")
          || !bridge_json_append_point2rd (buffer, segment->end_tangent))
        return false;
      break;
    default:
      if (!bridge_json_buffer_appendf (buffer, "\"type\":\"unknown\",\"curveType\":%d",
                                      (int)segment->curve_type))
        return false;
      break;
    }

  return bridge_json_buffer_append_char (buffer, '}');
}

static bool
bridge_json_append_hatch_segments (BridgeJsonBuffer *buffer,
                                  const Dwg_HATCH_Path *path)
{
  if (path->num_segs_or_paths > 0 && !path->segs)
    return false;

  if (!bridge_json_buffer_append_char (buffer, '['))
    return false;

  for (BITCODE_BL index = 0; index < path->num_segs_or_paths; index++)
    {
      if (index > 0 && !bridge_json_buffer_append_char (buffer, ','))
        return false;
      if (!bridge_json_append_hatch_segment (buffer, &path->segs[index]))
        return false;
    }

  return bridge_json_buffer_append_char (buffer, ']');
}

static bool
bridge_json_append_hatch_polyline_points (BridgeJsonBuffer *buffer,
                                         const Dwg_HATCH_Path *path)
{
  if (path->num_segs_or_paths > 0 && !path->polyline_paths)
    return false;

  if (!bridge_json_buffer_append_char (buffer, '['))
    return false;

  for (BITCODE_BL index = 0; index < path->num_segs_or_paths; index++)
    {
      if (index > 0 && !bridge_json_buffer_append_char (buffer, ','))
        return false;
      if (!bridge_json_append_point2rd (buffer, path->polyline_paths[index].point))
        return false;
    }

  return bridge_json_buffer_append_char (buffer, ']');
}

static bool
bridge_json_append_hatch_polyline_bulges (BridgeJsonBuffer *buffer,
                                         const Dwg_HATCH_Path *path)
{
  if (path->num_segs_or_paths > 0 && !path->polyline_paths)
    return false;

  if (!bridge_json_buffer_append_char (buffer, '['))
    return false;

  for (BITCODE_BL index = 0; index < path->num_segs_or_paths; index++)
    {
      if (index > 0 && !bridge_json_buffer_append_char (buffer, ','))
        return false;
      if (!bridge_json_buffer_appendf (
              buffer, "%.17g", path->polyline_paths[index].bulge))
        return false;
    }

  return bridge_json_buffer_append_char (buffer, ']');
}

static bool
bridge_json_append_hatch_contour (BridgeJsonBuffer *buffer, const Dwg_Object *obj,
                                 const Dwg_HATCH_Path *path, BITCODE_BL index)
{
  bool is_polyline;
  bool is_closed;

  if (!buffer || !obj || !path)
    return false;

  is_polyline = (path->flag & 2) != 0;
  is_closed = is_polyline ? (path->closed != 0) : ((path->flag & 0x20) == 0);

  if (!bridge_json_buffer_appendf (
          buffer,
          "{\"index\":%lld,\"type\":\"%s\",\"flag\":%lld,\"isClosed\":%s,\"boundaryHandles\":",
          (long long)index, is_polyline ? "polyline" : "segments",
          (long long)path->flag, is_closed ? "true" : "false")
      || !bridge_json_append_hatch_boundary_handles (
             buffer, obj, path->boundary_handles, path->num_boundary_handles))
    return false;

  if (is_polyline)
    {
      if (!bridge_json_buffer_append_cstr (buffer, ",\"points\":")
          || !bridge_json_append_hatch_polyline_points (buffer, path))
        return false;
      if (path->bulges_present)
        {
          if (!bridge_json_buffer_append_cstr (buffer, ",\"bulges\":")
              || !bridge_json_append_hatch_polyline_bulges (buffer, path))
            return false;
        }
    }
  else
    {
      if (!bridge_json_buffer_append_cstr (buffer, ",\"segments\":")
          || !bridge_json_append_hatch_segments (buffer, path))
        return false;
    }

  return bridge_json_buffer_append_char (buffer, '}');
}

char *
bridge_dwg_object_xrecord_xdata_json (const Dwg_Object *obj)
{
  const Dwg_Object_XRECORD *xrecord = bridge_dwg_xrecord_ptr (obj);
  const Dwg_Resbuf *rbuf;
  BridgeJsonBuffer buffer = { 0 };

  if (!xrecord)
    return NULL;

  rbuf = xrecord->xdata;
  if (!bridge_json_buffer_append_char (&buffer, '['))
    goto failed;

  for (BITCODE_BL index = 0; index < xrecord->num_xdata && rbuf; index++)
    {
      if (index > 0 && !bridge_json_buffer_append_char (&buffer, ','))
        goto failed;
      if (!bridge_json_buffer_append_char (&buffer, '[')
          || !bridge_json_buffer_appendf (&buffer, "%d,", rbuf->type)
          || !bridge_json_append_resbuf_value (&buffer, rbuf)
          || !bridge_json_buffer_append_char (&buffer, ']'))
        goto failed;
      rbuf = rbuf->nextrb;
    }

  if (!bridge_json_buffer_append_char (&buffer, ']'))
    goto failed;
  return buffer.data;

failed:
  free (buffer.data);
  return NULL;
}

static bool
bridge_json_append_block_connection (BridgeJsonBuffer *buffer,
                                     const Dwg_Object *obj, const char *property,
                                     BITCODE_BL index, BITCODE_BL code,
                                     BITCODE_T name, bool *is_first)
{
  if (!buffer || !is_first)
    return false;

  if (!*is_first && !bridge_json_buffer_append_char (buffer, ','))
    return false;
  *is_first = false;

  if (!bridge_json_buffer_append_char (buffer, '{'))
    return false;
  if (property
      && (!bridge_json_buffer_append_cstr (buffer, "\"property\":")
          || !bridge_json_buffer_append_json_string (buffer, property)
          || !bridge_json_buffer_append_char (buffer, ',')))
    return false;
  if (!bridge_json_buffer_appendf (
          buffer, "\"index\":%lld,\"code\":%lld,\"name\":",
          (long long)index, (long long)code))
    return false;

  if (obj && obj->parent && IS_FROM_TU_DWG (obj->parent) && name)
    {
      char *utf8 = bit_convert_TU ((BITCODE_TU)name);
      bool ok = bridge_json_buffer_append_json_string (buffer, utf8);
      free (utf8);
      if (!ok)
        return false;
    }
  else if (!bridge_json_buffer_append_json_string (buffer, name))
    return false;

  if (!bridge_json_buffer_append_char (buffer, '}'))
    return false;

  return true;
}

static bool
bridge_json_append_action_connections (BridgeJsonBuffer *buffer,
                                       const Dwg_Object *obj,
                                       const Dwg_BLOCKACTION_connectionpts *items,
                                       BITCODE_BL count, bool *is_first)
{
  if (!buffer || !items || !is_first)
    return false;

  for (BITCODE_BL index = 0; index < count; index++)
    {
      if (!bridge_json_append_block_connection (
              buffer, obj, NULL, index, items[index].code, items[index].name,
              is_first))
        return false;
    }

  return true;
}

static bool
bridge_json_append_parameter_connections (BridgeJsonBuffer *buffer,
                                          const Dwg_Object *obj,
                                          const char *property,
                                          const Dwg_BLOCKPARAMETER_PropInfo *info,
                                          bool *is_first)
{
  if (!buffer || !property || !info || !is_first)
    return false;

  if (info->num_connections > 1024)
    return false;
  if (info->num_connections > 0 && !info->connections)
    return false;

  for (BITCODE_BL index = 0; index < info->num_connections; index++)
    {
      if (!bridge_json_append_block_connection (
              buffer, obj, property, index, info->connections[index].code,
              info->connections[index].name, is_first))
        return false;
    }

  return true;
}

char *
bridge_dwg_object_block_connections_json (const Dwg_Object *obj)
{
  const char *name;
  bool first = true;
  bool matched = true;
  BridgeJsonBuffer buffer = { 0 };

  if (!obj || obj->supertype != DWG_SUPERTYPE_OBJECT || !obj->name)
    return NULL;

  name = obj->name;

  if (!bridge_json_buffer_append_char (&buffer, '['))
    goto failed;

#define APPEND_ACTION_CONNECTIONS(TYPE, COUNT)                                \
  do                                                                          \
    {                                                                         \
      const Dwg_Object_##TYPE *typed                                          \
          = (const Dwg_Object_##TYPE *)bridge_dwg_object_specific_ptr (obj);  \
      if (!typed                                                              \
          || !bridge_json_append_action_connections (&buffer, obj,             \
                                                     typed->conn_pts, COUNT,  \
                                                     &first))                 \
        goto failed;                                                          \
    }                                                                         \
  while (0)

#define APPEND_1PT_PARAMETER_CONNECTIONS(TYPE)                                \
  do                                                                          \
    {                                                                         \
      const Dwg_Object_##TYPE *typed                                          \
          = (const Dwg_Object_##TYPE *)bridge_dwg_object_specific_ptr (obj);  \
      if (!typed                                                              \
          || !bridge_json_append_parameter_connections (&buffer, obj, "prop1",\
                                                       &typed->prop1, &first) \
          || !bridge_json_append_parameter_connections (&buffer, obj, "prop2",\
                                                       &typed->prop2, &first))\
        goto failed;                                                          \
    }                                                                         \
  while (0)

#define APPEND_2PT_PARAMETER_CONNECTIONS(TYPE)                                \
  do                                                                          \
    {                                                                         \
      const Dwg_Object_##TYPE *typed                                          \
          = (const Dwg_Object_##TYPE *)bridge_dwg_object_specific_ptr (obj);  \
      if (!typed                                                              \
          || !bridge_json_append_parameter_connections (&buffer, obj, "prop1",\
                                                       &typed->prop1, &first) \
          || !bridge_json_append_parameter_connections (&buffer, obj, "prop2",\
                                                       &typed->prop2, &first) \
          || !bridge_json_append_parameter_connections (&buffer, obj, "prop3",\
                                                       &typed->prop3, &first) \
          || !bridge_json_append_parameter_connections (&buffer, obj, "prop4",\
                                                       &typed->prop4, &first))\
        goto failed;                                                          \
    }                                                                         \
  while (0)

  if (strcmp (name, "BLOCKARRAYACTION") == 0)
    APPEND_ACTION_CONNECTIONS (BLOCKARRAYACTION, 4);
  else if (strcmp (name, "BLOCKFLIPACTION") == 0)
    APPEND_ACTION_CONNECTIONS (BLOCKFLIPACTION, 4);
  else if (strcmp (name, "BLOCKMOVEACTION") == 0)
    APPEND_ACTION_CONNECTIONS (BLOCKMOVEACTION, 2);
  else if (strcmp (name, "BLOCKPOLARSTRETCHACTION") == 0)
    APPEND_ACTION_CONNECTIONS (BLOCKPOLARSTRETCHACTION, 6);
  else if (strcmp (name, "BLOCKROTATEACTION") == 0)
    APPEND_ACTION_CONNECTIONS (BLOCKROTATEACTION, 3);
  else if (strcmp (name, "BLOCKSCALEACTION") == 0)
    APPEND_ACTION_CONNECTIONS (BLOCKSCALEACTION, 5);
  else if (strcmp (name, "BLOCKSTRETCHACTION") == 0)
    APPEND_ACTION_CONNECTIONS (BLOCKSTRETCHACTION, 2);
  else if (strcmp (name, "BLOCKBASEPOINTPARAMETER") == 0)
    APPEND_1PT_PARAMETER_CONNECTIONS (BLOCKBASEPOINTPARAMETER);
  else if (strcmp (name, "BLOCKLOOKUPPARAMETER") == 0)
    APPEND_1PT_PARAMETER_CONNECTIONS (BLOCKLOOKUPPARAMETER);
  else if (strcmp (name, "BLOCKPOINTPARAMETER") == 0)
    APPEND_1PT_PARAMETER_CONNECTIONS (BLOCKPOINTPARAMETER);
  else if (strcmp (name, "BLOCKUSERPARAMETER") == 0)
    APPEND_1PT_PARAMETER_CONNECTIONS (BLOCKUSERPARAMETER);
  else if (strcmp (name, "BLOCKVISIBILITYPARAMETER") == 0)
    APPEND_1PT_PARAMETER_CONNECTIONS (BLOCKVISIBILITYPARAMETER);
  else if (strcmp (name, "BLOCKALIGNEDCONSTRAINTPARAMETER") == 0)
    APPEND_2PT_PARAMETER_CONNECTIONS (BLOCKALIGNEDCONSTRAINTPARAMETER);
  else if (strcmp (name, "BLOCKALIGNMENTPARAMETER") == 0)
    APPEND_2PT_PARAMETER_CONNECTIONS (BLOCKALIGNMENTPARAMETER);
  else if (strcmp (name, "BLOCKANGULARCONSTRAINTPARAMETER") == 0)
    APPEND_2PT_PARAMETER_CONNECTIONS (BLOCKANGULARCONSTRAINTPARAMETER);
  else if (strcmp (name, "BLOCKDIAMETRICCONSTRAINTPARAMETER") == 0)
    APPEND_2PT_PARAMETER_CONNECTIONS (BLOCKDIAMETRICCONSTRAINTPARAMETER);
  else if (strcmp (name, "BLOCKFLIPPARAMETER") == 0)
    APPEND_2PT_PARAMETER_CONNECTIONS (BLOCKFLIPPARAMETER);
  else if (strcmp (name, "BLOCKHORIZONTALCONSTRAINTPARAMETER") == 0)
    APPEND_2PT_PARAMETER_CONNECTIONS (BLOCKHORIZONTALCONSTRAINTPARAMETER);
  else if (strcmp (name, "BLOCKLINEARCONSTRAINTPARAMETER") == 0)
    APPEND_2PT_PARAMETER_CONNECTIONS (BLOCKLINEARCONSTRAINTPARAMETER);
  else if (strcmp (name, "BLOCKLINEARPARAMETER") == 0)
    APPEND_2PT_PARAMETER_CONNECTIONS (BLOCKLINEARPARAMETER);
  else if (strcmp (name, "BLOCKPOLARPARAMETER") == 0)
    APPEND_2PT_PARAMETER_CONNECTIONS (BLOCKPOLARPARAMETER);
  else if (strcmp (name, "BLOCKRADIALCONSTRAINTPARAMETER") == 0)
    APPEND_2PT_PARAMETER_CONNECTIONS (BLOCKRADIALCONSTRAINTPARAMETER);
  else if (strcmp (name, "BLOCKROTATIONPARAMETER") == 0)
    APPEND_2PT_PARAMETER_CONNECTIONS (BLOCKROTATIONPARAMETER);
  else if (strcmp (name, "BLOCKVERTICALCONSTRAINTPARAMETER") == 0)
    APPEND_2PT_PARAMETER_CONNECTIONS (BLOCKVERTICALCONSTRAINTPARAMETER);
  else if (strcmp (name, "BLOCKXYPARAMETER") == 0)
    APPEND_2PT_PARAMETER_CONNECTIONS (BLOCKXYPARAMETER);
  else
    matched = false;

#undef APPEND_ACTION_CONNECTIONS
#undef APPEND_1PT_PARAMETER_CONNECTIONS
#undef APPEND_2PT_PARAMETER_CONNECTIONS

  if (!matched)
    goto failed;

  if (!bridge_json_buffer_append_char (&buffer, ']'))
    goto failed;
  return buffer.data;

failed:
  free (buffer.data);
  return NULL;
}

char *
bridge_dwg_object_evaluation_graph_nodes_json (const Dwg_Object *obj)
{
  const Dwg_Object_EVALUATION_GRAPH *graph = bridge_dwg_evaluation_graph_ptr (obj);
  BridgeJsonBuffer buffer = { 0 };

  if (!graph || !graph->nodes)
    return NULL;

  if (!bridge_json_buffer_append_char (&buffer, '['))
    goto failed;

  for (BITCODE_BL i = 0; i < graph->num_nodes; i++)
    {
      const Dwg_EVAL_Node *node = &graph->nodes[i];
      const BITCODE_H evalexpr_ref = node->evalexpr;

      if (i > 0 && !bridge_json_buffer_append_char (&buffer, ','))
        goto failed;
      if (!bridge_json_buffer_append_char (&buffer, '{'))
        goto failed;
      if (!bridge_json_buffer_appendf (&buffer, "\"id\":%d,", (int)node->id))
        goto failed;
      if (!bridge_json_buffer_appendf (&buffer, "\"edge_flags\":%d,",
                                     (int)node->edge_flags))
        goto failed;
      if (!bridge_json_buffer_appendf (&buffer, "\"nextid\":%lld,",
                                     (long long)node->nextid))
        goto failed;
      if (!bridge_json_buffer_appendf (&buffer, "\"evalexpr\":\"%llX\",",
                                     (unsigned long long)(evalexpr_ref
                                                          ? evalexpr_ref->absolute_ref
                                                          : 0)))
        goto failed;
      if (!bridge_json_buffer_appendf (&buffer,
                                     "\"node\":[%lld,%lld,%lld,%lld]",
                                     (long long)node->node[0],
                                     (long long)node->node[1],
                                     (long long)node->node[2],
                                     (long long)node->node[3]))
        goto failed;
      if (!bridge_json_buffer_append_char (&buffer, '}'))
        goto failed;
    }

  if (!bridge_json_buffer_append_char (&buffer, ']'))
    goto failed;
  return buffer.data;

failed:
  free (buffer.data);
  return NULL;
}

char *
bridge_dwg_object_evaluation_graph_edges_json (const Dwg_Object *obj)
{
  const Dwg_Object_EVALUATION_GRAPH *graph = bridge_dwg_evaluation_graph_ptr (obj);
  BridgeJsonBuffer buffer = { 0 };

  if (!graph || !graph->edges)
    return NULL;

  if (!bridge_json_buffer_append_char (&buffer, '['))
    goto failed;

  for (BITCODE_BL i = 0; i < graph->num_edges; i++)
    {
      const Dwg_EVAL_Edge *edge = &graph->edges[i];

      if (i > 0 && !bridge_json_buffer_append_char (&buffer, ','))
        goto failed;
      if (!bridge_json_buffer_append_char (&buffer, '{'))
        goto failed;
      if (!bridge_json_buffer_appendf (&buffer, "\"id\":%d,", (int)edge->id))
        goto failed;
      if (!bridge_json_buffer_appendf (&buffer, "\"nextid\":%lld,",
                                     (long long)edge->nextid))
        goto failed;
      if (!bridge_json_buffer_appendf (&buffer, "\"e1\":%lld,\"e2\":%lld,\"e3\":%lld,",
                                     (long long)edge->e1,
                                     (long long)edge->e2,
                                     (long long)edge->e3))
        goto failed;
      if (!bridge_json_buffer_appendf (&buffer,
                                     "\"out_edge\":[%lld,%lld,%lld,%lld,%lld]",
                                     (long long)edge->out_edge[0],
                                     (long long)edge->out_edge[1],
                                     (long long)edge->out_edge[2],
                                     (long long)edge->out_edge[3],
                                     (long long)edge->out_edge[4]))
        goto failed;
      if (!bridge_json_buffer_append_char (&buffer, '}'))
        goto failed;
    }

  if (!bridge_json_buffer_append_char (&buffer, ']'))
    goto failed;
  return buffer.data;

failed:
  free (buffer.data);
  return NULL;
}

char *
bridge_dwg_object_hatch_contours_json (const Dwg_Object *obj)
{
  const Dwg_Entity_HATCH *hatch = bridge_dwg_hatch_ptr (obj);
  BridgeJsonBuffer buffer = { 0 };

  if (!hatch || (hatch->num_paths > 0 && !hatch->paths))
    return NULL;

  if (!bridge_json_buffer_append_char (&buffer, '['))
    goto failed;

  for (BITCODE_BL index = 0; index < hatch->num_paths; index++)
    {
      if (index > 0 && !bridge_json_buffer_append_char (&buffer, ','))
        goto failed;
      if (!bridge_json_append_hatch_contour (&buffer, obj, &hatch->paths[index], index))
        goto failed;
    }

  if (!bridge_json_buffer_append_char (&buffer, ']'))
    goto failed;
  return buffer.data;

failed:
  free (buffer.data);
  return NULL;
}

char *
bridge_dwg_object_lwpolyline_geometry_json (const Dwg_Object *obj)
{
  const Dwg_Entity_LWPOLYLINE *polyline = bridge_dwg_lwpolyline_ptr (obj);
  BridgeJsonBuffer buffer = { 0 };

  if (!polyline || (polyline->num_points > 0 && !polyline->points)
      || (polyline->num_bulges > 0 && !polyline->bulges))
    return NULL;
  if (!bridge_json_buffer_append_cstr (&buffer, "{\"points\":["))
    goto failed;
  for (BITCODE_BL index = 0; index < polyline->num_points; index++)
    {
      if ((index > 0 && !bridge_json_buffer_append_char (&buffer, ','))
          || !bridge_json_append_point2rd (&buffer, polyline->points[index]))
        goto failed;
    }
  if (!bridge_json_buffer_append_cstr (&buffer, "],\"bulges\":["))
    goto failed;
  for (BITCODE_BL index = 0; index < polyline->num_points; index++)
    {
      double bulge = index < polyline->num_bulges ? polyline->bulges[index] : 0.0;
      if ((index > 0 && !bridge_json_buffer_append_char (&buffer, ','))
          || !bridge_json_buffer_appendf (&buffer, "%.17g", bulge))
        goto failed;
    }
  if (!bridge_json_buffer_append_char (&buffer, ']')
      || !bridge_json_buffer_append_char (&buffer, '}'))
    goto failed;
  return buffer.data;

failed:
  free (buffer.data);
  return NULL;
}

char *
bridge_dwg_object_spline_geometry_json (const Dwg_Object *obj)
{
  const Dwg_Entity_SPLINE *spline = bridge_dwg_spline_ptr (obj);
  BridgeJsonBuffer buffer = { 0 };

  if (!spline || (spline->num_knots > 0 && !spline->knots)
      || (spline->num_ctrl_pts > 0 && !spline->ctrl_pts)
      || (spline->num_fit_pts > 0 && !spline->fit_pts))
    return NULL;
  if (!bridge_json_buffer_appendf (
          &buffer,
          "{\"scenario\":%d,\"degree\":%d,\"isRational\":%s,\"isPeriodic\":%s,\"isClosed\":%s,\"knots\":[",
          (int)spline->scenario, (int)spline->degree,
          spline->rational ? "true" : "false",
          spline->periodic ? "true" : "false",
          spline->closed_b ? "true" : "false"))
    goto failed;
  for (BITCODE_BL index = 0; index < spline->num_knots; index++)
    {
      if ((index > 0 && !bridge_json_buffer_append_char (&buffer, ','))
          || !bridge_json_buffer_appendf (&buffer, "%.17g", spline->knots[index]))
        goto failed;
    }
  if (!bridge_json_buffer_append_cstr (&buffer, "],\"controlPoints\":["))
    goto failed;
  for (BITCODE_BL index = 0; index < spline->num_ctrl_pts; index++)
    {
      const Dwg_SPLINE_control_point *control = &spline->ctrl_pts[index];
      if ((index > 0 && !bridge_json_buffer_append_char (&buffer, ','))
          || !bridge_json_buffer_appendf (
                 &buffer,
                 "{\"point\":[%.17g,%.17g,%.17g],\"weight\":%.17g}",
                 control->x, control->y, control->z,
                 spline->rational ? control->w : 1.0))
        goto failed;
    }
  if (!bridge_json_buffer_append_cstr (&buffer, "],\"fitPoints\":["))
    goto failed;
  for (BITCODE_BL index = 0; index < spline->num_fit_pts; index++)
    {
      const BITCODE_3DPOINT point = spline->fit_pts[index];
      if ((index > 0 && !bridge_json_buffer_append_char (&buffer, ','))
          || !bridge_json_buffer_appendf (&buffer, "[%.17g,%.17g,%.17g]",
                                         point.x, point.y, point.z))
        goto failed;
    }
  if (!bridge_json_buffer_appendf (
          &buffer,
          "],\"startTangent\":[%.17g,%.17g,%.17g],\"endTangent\":[%.17g,%.17g,%.17g]}",
          spline->beg_tan_vec.x, spline->beg_tan_vec.y, spline->beg_tan_vec.z,
          spline->end_tan_vec.x, spline->end_tan_vec.y, spline->end_tan_vec.z))
    goto failed;
  return buffer.data;

failed:
  free (buffer.data);
  return NULL;
}

char *
bridge_dwg_object_wipeout_geometry_json (const Dwg_Object *obj)
{
  const Dwg_Entity_WIPEOUT *wipeout = bridge_dwg_wipeout_ptr (obj);
  BridgeJsonBuffer buffer = { 0 };

  if (!wipeout || wipeout->num_clip_verts > 5000
      || (wipeout->num_clip_verts > 0 && !wipeout->clip_verts))
    return NULL;
  if (!bridge_json_buffer_append_cstr (&buffer, "{\"clip_verts\":["))
    goto failed;
  for (BITCODE_BL index = 0; index < wipeout->num_clip_verts; index++)
    {
      if ((index > 0 && !bridge_json_buffer_append_char (&buffer, ','))
          || !bridge_json_append_point2rd (&buffer, wipeout->clip_verts[index]))
        goto failed;
    }
  if (!bridge_json_buffer_append_cstr (&buffer, "]}"))
    goto failed;
  return buffer.data;

failed:
  free (buffer.data);
  return NULL;
}

void
bridge_dwg_string_free (char *value)
{
  if (value)
    free (value);
}

void
bridge_dwg_field_value_free(BridgeDwgFieldValue *value)
{
  if (!value)
    return;
  if (value->owns_string && value->string_value)
    free(value->string_value);
  memset(value, 0, sizeof(*value));
}
