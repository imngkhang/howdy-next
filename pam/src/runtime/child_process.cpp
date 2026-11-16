#ifndef _GNU_SOURCE
#	define _GNU_SOURCE
#endif

#include "runtime/child_process.hpp"

#include "support/eintr.hpp"

#include <cerrno>

#include <sys/types.h>
#include <sys/wait.h>

// Keep C linkage explicit for glibc headers without a C++ linkage guard.
extern "C" {
#include <sys/pidfd.h>
}

namespace {

	constexpr unsigned int kExitStatusShift   = 8U;
	constexpr unsigned int kExitStatusMask    = 0xffU;
	constexpr unsigned int kTermSignalMask    = 0x7fU;
	constexpr unsigned int kCoreDumpStatusBit = 0x80U;

	// waitid reports the outcome in pieces; callers use the classic WIFEXITED/WIFSIGNALED form.
	auto WaitStatusFromSiginfo(const siginfo_t &info) noexcept -> int {
		const auto value = static_cast<unsigned int>(info.si_status);
		if (info.si_code == CLD_EXITED) {
			return static_cast<int>((value & kExitStatusMask) << kExitStatusShift);
		}
		const unsigned int core_bit = info.si_code == CLD_DUMPED ? kCoreDumpStatusBit : 0U;
		return static_cast<int>((value & kTermSignalMask) | core_bit);
	}

}  // namespace

namespace howdy::pam::runtime {

	auto OpenChildPidfd(pid_t child_pid) noexcept -> int {
		return pidfd_open(child_pid, 0);
	}

	auto WaitChildPidfd(int pidfd, bool block, int *status) noexcept -> ChildState {
		if (pidfd < 0 || status == nullptr) {
			errno = EINVAL;
			return ChildState::kError;
		}
		siginfo_t info{};
		const int options = block ? WEXITED : WEXITED | WNOHANG;
		if (howdy::native::RetryOnEintr([&] -> int {
			    info = {};
			    return waitid(P_PIDFD, static_cast<id_t>(pidfd), &info, options);
		    }) != 0) {
			return ChildState::kError;
		}
		// WNOHANG with no state change succeeds and leaves si_pid zero.
		if (info.si_pid == 0) {
			return ChildState::kRunning;
		}
		*status = WaitStatusFromSiginfo(info);
		return ChildState::kReaped;
	}

	auto SignalChildPidfd(int pidfd, int signal) noexcept -> bool {
		return pidfd_send_signal(pidfd, signal, nullptr, 0) == 0 || errno == ESRCH;
	}

}  // namespace howdy::pam::runtime
