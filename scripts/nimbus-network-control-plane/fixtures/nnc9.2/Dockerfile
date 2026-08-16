FROM scratch
COPY rootfs/bin/busybox /bin/busybox
COPY rootfs/lib/ld-linux-x86-64.so.2 /lib64/ld-linux-x86-64.so.2
COPY rootfs/lib/libc.so.6 /lib/x86_64-linux-gnu/libc.so.6
COPY rootfs/lib/libm.so.6 /lib/x86_64-linux-gnu/libm.so.6
COPY rootfs/lib/libresolv.so.2 /lib/x86_64-linux-gnu/libresolv.so.2
COPY lifecycle.sh /lifecycle.sh
ENTRYPOINT ["/bin/busybox", "sh", "/lifecycle.sh"]
EXPOSE 8080/tcp
