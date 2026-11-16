#pragma once

#include <cstdint>

#include <sys/types.h>

namespace howdy::pam::runtime {

	enum class ChildState : std::uint8_t {
		kRunning,
		kReaped,
		kError,
	};

	// A pidfd names one process for as long as it is open, so signalling and reaping through it
	// can never reach an unrelated process that later reuses the child's PID. That reuse is
	// possible when the PAM application reaps children itself.

	// Close-on-exec descriptor for `child_pid`, or -1 with errno set. Only meaningful while the
	// caller knows the PID is still its unreaped child; WaitChildPidfd reports ECHILD otherwise.
	// <sys/pidfd.h> has no include guard, so this is the one place that includes it.
	[[nodiscard]] __attribute__((visibility("hidden"))) auto
	OpenChildPidfd(pid_t child_pid) noexcept -> int;

	// Reaps the child behind `pidfd`, waiting for it to exit when `block` is set. `status`
	// receives a wait(2)-style status once reaped. kError leaves errno set; ECHILD means the
	// descriptor does not name an unreaped child of this process.
	[[nodiscard]] __attribute__((visibility("hidden"))) auto WaitChildPidfd(int pidfd, bool block,
	                                                                        int *status) noexcept
	    -> ChildState;

	// True when the signal was sent or the process is already gone; otherwise errno is set.
	[[nodiscard]] __attribute__((visibility("hidden"))) auto SignalChildPidfd(int pidfd,
	                                                                          int signal) noexcept
	    -> bool;

}  // namespace howdy::pam::runtime
