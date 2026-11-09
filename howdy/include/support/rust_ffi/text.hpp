#pragma once

// Conversions between C++ strings and the Rust FFI byte view, shared by the FFI wrappers. Only
// targets linked through howdy_link_rust_ffi() can include this header.
#include "howdy_ffi.h"

#include <string_view>

namespace howdy::native::rust_ffi {

	inline auto View(std::string_view text) -> HowdyStr {
		return {.ptr = text.data(), .len = text.size()};
	}

	inline auto Text(HowdyStr text) -> std::string_view {
		return text.len == 0 ? std::string_view{} : std::string_view(text.ptr, text.len);
	}

}  // namespace howdy::native::rust_ffi
