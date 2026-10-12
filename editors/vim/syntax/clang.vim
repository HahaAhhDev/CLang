if exists("b:current_syntax")
  finish
endif

syn match clangComment "#.*$"
syn match clangVar "var:[A-Za-z_][A-Za-z0-9_]*"
syn match clangFuncDef "fn::[A-Za-z_][A-Za-z0-9_]*"
syn match clangKeyword "\<\(if\|else\|for\|in\|return\|run\)\>"
syn match clangBuiltin "\<\(emit\|cap_first\|lower\|upper\|contains\|len\|split\|join\|trim\|replace\|starts_with\|ends_with\|file_read\|file_write\|json_parse\|json_dump\|http_get\|http_post\|http_serve\|db_put\|db_get\|db_list\|tbl_put\|tbl_get\|tbl_list\|tbl_del\|pmap\|exec\|env\|cli_args\|time_now\|sleep_ms\|rand_seed\|rand_int\|sort\|push\|mk_user\)\>"
syn region clangString start=+"+ skip=+\\\\\|\\"+ end=+"+ contains=clangInterp
syn match clangInterp "{\[^}]\\+}" contained
syn match clangNumber "\<[0-9]\+\(\.[0-9]\+\)\?\>"

hi def link clangComment Comment
hi def link clangVar Identifier
hi def link clangFuncDef Function
hi def link clangKeyword Keyword
hi def link clangBuiltin Builtin
hi def link clangString String
hi def link clangInterp Special
hi def link clangNumber Number

let b:current_syntax = "clang"
