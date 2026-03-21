#include "bridge.h"

#include <bits.h>
#include <stdlib.h>
#include <stdio.h>
#include <stdarg.h>
#include <string.h>

Dwg_Data *
bridge_dwg_data_new(void)
{
  return (Dwg_Data *)calloc(1, sizeof(Dwg_Data));
}

int
bridge_dwg_data_read_file(Dwg_Data *dwg, const char *filename)
{
  if (!dwg || !filename)
    return DWG_ERR_IOERROR;
  return dwg_read_file(filename, dwg);
}

BITCODE_BL
bridge_dwg_data_num_objects(const Dwg_Data *dwg)
{
  return dwg ? dwg->num_objects : 0;
}

void
bridge_dwg_data_free(Dwg_Data *dwg)
{
  if (!dwg)
    return;
  dwg_free(dwg);
  free(dwg);
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
static bool bridge_json_append_hex_string (BridgeJsonBuffer *buffer,
                                          const unsigned char *bytes,
                                          size_t length);
static bool bridge_json_append_resbuf_value (BridgeJsonBuffer *buffer,
                                            const Dwg_Resbuf *rbuf);

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
bridge_type_matches(const char *type, const char *candidate)
{
  return type && strcmp(type, candidate) == 0;
}

bool
bridge_dwg_object_read_field(const Dwg_Object *obj, const char *fieldname,
                            BridgeDwgFieldValue *out)
{
  Dwg_DYNAPI_field fp;
  const Dwg_DYNAPI_field *field;
  bool is_common;
  memset(out, 0, sizeof(*out));

  if (!obj || !fieldname || !out)
    return false;

  field = bridge_dwg_common_field(obj, fieldname);
  is_common = field != NULL;
  if (!field)
    {
      field = dwg_dynapi_entity_field(obj->name, fieldname);
      if (!field)
        return false;
    }
  memcpy(&fp, field, sizeof(fp));

  if (fp.is_string)
    return bridge_read_string_field(obj, fieldname, is_common, out, &fp);

  if (strchr(fp.type, '*'))
    return false;

  if (strchr(fp.type, 'H'))
    {
      BITCODE_H ref = NULL;
      if (!bridge_read_raw_field(obj, fieldname, is_common, &ref, &fp))
        return false;
      out->kind = BRIDGE_DWG_FIELD_HANDLE;
      out->handle_value = ref ? ref->absolute_ref : 0;
      return true;
    }

  if (bridge_type_matches(fp.type, "2RD") || bridge_type_matches(fp.type, "2BD")
      || bridge_type_matches(fp.type, "2DD"))
    {
      dwg_point_2d point;
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
      if (!bridge_read_raw_field(obj, fieldname, is_common, &color, &fp))
        return false;
      out->kind = BRIDGE_DWG_FIELD_INTEGER;
      out->integer_value = color.index;
      return true;
    }

  if (bridge_type_matches(fp.type, "3BD") || bridge_type_matches(fp.type, "3BD_1")
      || bridge_type_matches(fp.type, "3RD") || bridge_type_matches(fp.type, "3DD")
      || bridge_type_matches(fp.type, "BE"))
    {
      dwg_point_3d point;
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
      if (!bridge_read_raw_field(obj, fieldname, is_common, &value, &fp))
        return false;
      out->kind = BRIDGE_DWG_FIELD_DOUBLE;
      out->double_value = value;
      return true;
    }

  if (bridge_type_matches(fp.type, "B") || bridge_type_matches(fp.type, "BB"))
    {
      BITCODE_B value = 0;
      if (!bridge_read_raw_field(obj, fieldname, is_common, &value, &fp))
        return false;
      out->kind = BRIDGE_DWG_FIELD_BOOL;
      out->integer_value = value != 0;
      return true;
    }

  if (bridge_type_matches(fp.type, "RC"))
    {
      BITCODE_RC value = 0;
      if (!bridge_read_raw_field(obj, fieldname, is_common, &value, &fp))
        return false;
      out->kind = BRIDGE_DWG_FIELD_INTEGER;
      out->integer_value = value;
      return true;
    }

  if (bridge_type_matches(fp.type, "RS") || bridge_type_matches(fp.type, "BS")
      || bridge_type_matches(fp.type, "BSd"))
    {
      BITCODE_BS value = 0;
      if (!bridge_read_raw_field(obj, fieldname, is_common, &value, &fp))
        return false;
      out->kind = BRIDGE_DWG_FIELD_INTEGER;
      out->integer_value = value;
      return true;
    }

  if (bridge_type_matches(fp.type, "RL") || bridge_type_matches(fp.type, "BL"))
    {
      BITCODE_BL value = 0;
      if (!bridge_read_raw_field(obj, fieldname, is_common, &value, &fp))
        return false;
      out->kind = BRIDGE_DWG_FIELD_INTEGER;
      out->integer_value = value;
      return true;
    }

  if (bridge_type_matches(fp.type, "BLL") || bridge_type_matches(fp.type, "RLL"))
    {
      BITCODE_RLL value = 0;
      if (!bridge_read_raw_field(obj, fieldname, is_common, &value, &fp))
        return false;
      out->kind = BRIDGE_DWG_FIELD_INTEGER;
      out->integer_value = (long long)value;
      return true;
    }

  if (bridge_type_matches(fp.type, "BLd"))
    {
      BITCODE_BLd value = 0;
      if (!bridge_read_raw_field(obj, fieldname, is_common, &value, &fp))
        return false;
      out->kind = BRIDGE_DWG_FIELD_INTEGER;
      out->integer_value = (long long)value;
      return true;
    }

  return false;
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
