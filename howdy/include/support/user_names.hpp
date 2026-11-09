#pragma once

#include <algorithm>
#include <cctype>
#include <cstddef>
#include <string_view>

namespace howdy::native {

	inline constexpr auto kInvalidUserNameMessage =
	    "Invalid user name. Refusing to use an unsafe model file path.";

	// Longest user name whose `<user>.model` file name fits NAME_MAX (255). user_model_path.hpp
	// static_asserts this against the extension, so every entry point rejects longer names.
	inline constexpr std::size_t kMaxModelUserNameBytes = 249;

	inline auto IsValidModelUserName(const std::string_view user) -> bool {
		if (user.empty() || user == "." || user == ".." || user.size() > kMaxModelUserNameBytes) {
			return false;
		}

		if (user.front() == '.' || user.front() == '-' || user.contains("..")) {
			return false;
		}

		return std::ranges::all_of(user, [](const char raw) -> bool {
			const auto ch = static_cast<unsigned char>(raw);
			return raw != '/' && raw != '\\' && std::isspace(ch) == 0 && std::iscntrl(ch) == 0;
		});
	}

}  // namespace howdy::native
