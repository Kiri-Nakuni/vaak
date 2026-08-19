; PraTeX 埋め込みスクリプト言語のハイライト。
; 後の規則ほど優先されるので、汎用 → 特殊 の順に並べる。
;
; 捕捉名は Zed のテーマが定義しているものに寄せてある。ドット区切りは
; 前方一致でフォールバックするので、`@keyword.control.exception` を定義
; していないテーマでは `@keyword` の色になる。

(identifier) @variable

; ---- コメント・リテラル ----

(comment) @comment
(integer) @number
(string) @string
(string (escape_sequence) @string.escape)

; ---- 型 ----

(primitive_type) @type
(type_identifier) @type
(type_definition name: (type_identifier) @type)
(field_declaration name: (identifier) @property)
(field_expression field: (identifier) @property)

; ---- 束縛・関数 ----

(function_definition name: (identifier) @function.definition)
(parameter name: (identifier) @variable.parameter)
(call_expression function: (identifier) @function)
(call_expression function: (field_expression field: (identifier) @function.method))

(let_binding name: (identifier) @variable)
(nfor_expression binder: (identifier) @variable)
(ifor_statement binder: (identifier) @variable)

; 入出力プリミティブ（§4.5）
((identifier) @function.builtin
  (#any-of? @function.builtin "read" "read_lines" "print"))

; レジスタ層（§3.7）。ヒープ確保とはコスト特性も巻き戻し挙動も違う
; 別の記憶モデルなので、通常の変数と見分けられるようにする。
((identifier) @variable.special
  (#match? @variable.special "^count(i64|u64|i32|u32|f32|f64|f80|u8|u16)$"))

; ---- キーワード ----

["let" "typ"] @keyword
["if" "else" "match" "while" "loop" "nfor" "ifor" "comp" "in"] @keyword
"break" @keyword
(wildcard) @constant

; ---- jump：この言語の中心なので他のキーワードと区別する ----
;
; §4.1 の3行がすべて：comefrom が位置を保存し、goto が label の直前まで
; 読み飛ばし、label は保存されている comefrom があればその直後へ飛ぶ。

["goto" "label" "comefrom"] @keyword.control.exception
(label_identifier) @label

; ---- アトリビュート ----
;
; D-18r: 行末で終端し、次の行の評価方法を変更する。

(attribute) @punctuation.special
(attribute (attribute_name) @attribute)

; ---- 演算子 ----

; §4.6: 束縛演算子は3つとも意味が違う。`:=` は新 thunk の生成という
; 根源的操作、`&=` は thunk の共有、`^=` は未評価の計算の複製。
; 後ろ2つは他言語に対応物がないので、代入と同じ色に埋もれさせない。
":=" @operator
["&=" "^="] @keyword.operator

; D-17: `+=` は `:=` の糖衣ではない。左辺の現在値を即時強制してから演算する。
["+=" "-=" "*=" "/="] @operator

; D-8: `->` は宣言位置でも呼出位置でも返り値型を名指す。
"->" @keyword.operator

; D-7: 剰余は `mod`（`%` はコメント専用）。
"mod" @keyword.operator

["+" "-" "*" "/" "==" "!=" "<" ">" "<=" ">=" "&&" "||" "!" ".."] @operator
"=>" @punctuation.delimiter

; ---- 区切り ----

["(" ")" "[" "]"] @punctuation.bracket
["{" "}"] @punctuation.bracket
[";" "," ":" "."] @punctuation.delimiter
