(function_definition
  "fn" @context
  name: (identifier) @name) @item

(type_definition
  "typ" @context
  name: (type_identifier) @name) @item

; jump のラベルはこの言語の制御構造そのものなので outline に出す
(label_statement
  "label" @context
  label: (label_identifier) @name) @item

(comefrom_statement
  "comefrom" @context
  label: (label_identifier) @name) @item
