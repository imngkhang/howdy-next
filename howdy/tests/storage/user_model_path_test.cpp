#include "storage/user_model_path.hpp"
#include "test_support.hpp"

#include <filesystem>
#include <string>

#include <linux/limits.h>

namespace {

	using howdy::test::Expect;

}  // namespace

auto main() -> int {
	bool ok = true;

	const std::filesystem::path models_dir = "/trusted/models";
	ok &= Expect(howdy::native::ResolveUserModelPath(models_dir, "alice") ==
	                 models_dir / "alice.model",
	             "safe username resolves inside models directory");
	ok &= Expect(!howdy::native::ResolveUserModelPath(models_dir, "../alice").has_value(),
	             "unsafe username cannot escape models directory");
	ok &= Expect(!howdy::native::ResolveUserModelPath(models_dir, "alice..bob").has_value(),
	             "dot-dot username input is rejected before model path construction");

	const auto longest = std::string(NAME_MAX - howdy::native::kUserModelFileExtension.size(), 'u');
	ok &= Expect(howdy::native::ResolveUserModelPath(models_dir, longest).has_value(),
	             "username whose model file name is exactly NAME_MAX resolves");
	ok &= Expect(!howdy::native::ResolveUserModelPath(models_dir, longest + "u").has_value(),
	             "username whose model file name exceeds NAME_MAX is rejected");

	return ok ? 0 : 1;
}
