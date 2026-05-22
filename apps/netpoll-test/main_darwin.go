package main

import (
	"errors"
	"fmt"
	"net"
	"syscall"
	"time"

	"golang.org/x/sync/errgroup"
)

func main() {
	err := supported()
	if err != nil {
		fmt.Printf("netpoll NOT supported: %v\n", err)
	} else {
		fmt.Println("netpoll supported — all checks passed")
	}
}

func socketFD(conn net.Conn) int {
	if con, ok := conn.(syscall.Conn); ok {
		raw, err := con.SyscallConn()
		if err != nil {
			return 0
		}
		sfd := 0
		raw.Control(func(fd uintptr) {
			sfd = int(fd)
		})
		return sfd
	}
	return 0
}

func supported() error {
	pollTimeout := 10 * time.Millisecond

	// Create kqueue fd (same as netpoll.NewPoller)
	kqFd, err := syscall.Kqueue()
	if err != nil {
		return fmt.Errorf("kqueue not supported: %w", err)
	}
	defer syscall.Close(kqFd)

	_, err = syscall.Kevent(kqFd, []syscall.Kevent_t{{
		Ident:  0,
		Filter: syscall.EVFILT_USER,
		Flags:  syscall.EV_ADD | syscall.EV_CLEAR,
	}}, nil, nil)
	if err != nil {
		return fmt.Errorf("kevent init failed: %w", err)
	}

	ts := syscall.NsecToTimespec(pollTimeout.Nanoseconds())

	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		return fmt.Errorf("failed to create listener: %w", err)
	}
	defer ln.Close()

	fmt.Println("listening on", ln.Addr().String())

	var addConnErrGroup errgroup.Group

	addConnErrGroup.Go(func() error {
		conn, err := ln.Accept()
		fmt.Println("Accepted")
		if err != nil {
			return fmt.Errorf("failed to accept connection: %w", err)
		}

		// Add connection to kqueue (same as KQueue.Add)
		fd := socketFD(conn)
		if e := syscall.SetNonblock(fd, true); e != nil {
			return errors.New("SetNonblock failed")
		}

		changes := []syscall.Kevent_t{{
			Ident:  uint64(fd),
			Flags:  syscall.EV_ADD | syscall.EV_EOF,
			Filter: syscall.EVFILT_READ,
		}}

		// Wait for events (same as KQueue.Wait)
		events := make([]syscall.Kevent_t, 1)
		n, err := syscall.Kevent(kqFd, changes, events, &ts)
		if err != nil {
			return fmt.Errorf("failed to wait for events: %w", err)
		}
		fmt.Printf("kqueue returned %d events\n", n)

		conn.Close()
		return nil
	})

	var dialErrGroup errgroup.Group

	dialErrGroup.Go(func() error {
		conn, err := net.Dial("tcp", ln.Addr().String())
		fmt.Println("point C")
		if err != nil {
			return err
		}
		fmt.Println("point D")
		defer conn.Close()

		if err := addConnErrGroup.Wait(); err != nil {
			return err
		}

		fmt.Println("point A")
		_, err = conn.Write([]byte("hello"))
		fmt.Println("point B")
		if err != nil {
			return err
		}

		return nil
	})

	return dialErrGroup.Wait()
}
